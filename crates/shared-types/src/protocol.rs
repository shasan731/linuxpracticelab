//! Control-channel protocol between the host application and the guest LinuxLab Agent.
//!
//! Transport is one newline-delimited JSON object per message over a loopback TCP socket
//! that QEMU forwards into the guest. Every request carries the shared `token` that the
//! host generated for this VM run; the agent drops any framed message whose token does not
//! match, which is what keeps other local processes off the channel (spec 7.2, 21.4).

use crate::lesson::Validator;
use crate::validation::{CheckOutcome, TaskValidation};
use serde::{Deserialize, Serialize};

fn default_subject_user() -> String {
    "student".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestEnvelope {
    /// Correlates a response with its request.
    pub id: u64,
    pub token: String,
    pub request: AgentRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseEnvelope {
    pub id: u64,
    pub response: AgentResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AgentRequest {
    /// Liveness and version handshake. The host polls this until the guest is Ready.
    Ping,
    /// Prepares a lesson environment: runs the setup script, materialises fixtures.
    PrepareLesson {
        lesson_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        setup_script: Option<String>,
        #[serde(default)]
        fixtures: Vec<String>,
        #[serde(default)]
        namespaces: Vec<String>,
        #[serde(default)]
        sudo_allowed: bool,
    },
    /// Runs the validators for one task and reports per-check outcomes.
    ValidateTask {
        lesson_id: String,
        task_id: String,
        validators: Vec<Validator>,
        #[serde(default = "default_subject_user")]
        subject_user: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attempt_started_at: Option<i64>,
    },
    /// Reverts the lesson environment to its prepared state without rebooting.
    ResetLesson {
        lesson_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reset_script: Option<String>,
    },
    /// Records a filesystem checkpoint the lesson can roll back to.
    Checkpoint { name: String },
    /// Safe diagnostics for the Environment panel: no command history, no file contents.
    Diagnostics,
    /// Directory listing for the File Tree panel, confined to allowed roots.
    ListDirectory {
        path: String,
        #[serde(default)]
        include_hidden: bool,
    },
    /// Reported package and image versions.
    Versions,
    /// Tells the guest how large the terminal is.
    ///
    /// A serial console carries no window-size information, so without this the guest keeps
    /// its default 80x24 and long lines wrap in the wrong place after the learner resizes the
    /// panel. The agent applies the size to the console tty, which also raises SIGWINCH so
    /// full-screen programs such as `less`, `htop` and `nano` redraw correctly.
    SetTerminalSize { rows: u16, cols: u16 },
    /// Requests a clean guest shutdown from inside, before the host powers QEMU off.
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentResponse {
    Pong {
        agent_version: String,
        kernel: String,
        image_version: String,
        uptime_seconds: u64,
    },
    LessonPrepared {
        lesson_id: String,
        #[serde(default)]
        warnings: Vec<String>,
    },
    TaskValidated(TaskValidation),
    LessonReset {
        lesson_id: String,
    },
    CheckpointCreated {
        name: String,
    },
    Diagnostics(GuestDiagnostics),
    DirectoryListing {
        path: String,
        entries: Vec<DirEntryInfo>,
    },
    Versions {
        image_version: String,
        agent_version: String,
        packages: Vec<PackageInfo>,
    },
    TerminalResized {
        rows: u16,
        cols: u16,
    },
    ShuttingDown,
    /// The agent understood the frame but could not carry it out.
    Error {
        message: String,
        #[serde(default)]
        retriable: bool,
    },
}

// `f32` deliberately does not implement `Eq` because NaN is not equal to itself.
// Diagnostics only need structural/round-trip comparisons, for which `PartialEq` is correct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GuestDiagnostics {
    pub hostname: String,
    pub kernel: String,
    pub uptime_seconds: u64,
    pub load_average: [f32; 3],
    pub memory_total_kb: u64,
    pub memory_available_kb: u64,
    pub root_disk_used_percent: u8,
    pub root_inodes_used_percent: u8,
    pub failed_units: Vec<String>,
    pub listening_ports: Vec<u16>,
    pub current_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryInfo {
    pub name: String,
    pub file_type: String,
    pub size: u64,
    pub mode: String,
    pub owner: String,
    pub group: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
}

/// Convenience for the agent when a request cannot even be parsed into an outcome list.
pub fn error_outcome(kind: &str, message: impl Into<String>) -> CheckOutcome {
    CheckOutcome::error(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_externally_tagged_by_op() {
        let env = RequestEnvelope {
            id: 7,
            token: "secret".into(),
            request: AgentRequest::Ping,
        };
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains(r#""op":"ping""#), "got {json}");

        let back: RequestEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn validate_task_roundtrips_with_validators() {
        let req = AgentRequest::ValidateTask {
            lesson_id: "filesystem.navigation.04".into(),
            task_id: "task-1".into(),
            validators: vec![Validator::new("current_directory").with("path", "/home/student")],
            subject_user: "student".into(),
            attempt_started_at: Some(1_700_000_000),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""lessonId":"filesystem.navigation.04""#));
        assert!(json.contains(r#""subjectUser":"student""#));
        assert!(!json.contains("lesson_id"));
        let back: AgentRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn response_variant_fields_are_camel_case_on_the_wire() {
        let response = AgentResponse::Pong {
            agent_version: "0.1.0".into(),
            kernel: "6.12".into(),
            image_version: "test-image".into(),
            uptime_seconds: 5,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(r#""agentVersion":"0.1.0""#));
        assert!(json.contains(r#""imageVersion":"test-image""#));
        assert!(json.contains(r#""uptimeSeconds":5"#));
        assert!(!json.contains("agent_version"));
    }
}
