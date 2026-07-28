//! Validation request and result types.
//!
//! The engine reports one outcome per validator rather than a single pass/fail so the
//! lesson player can say *which* part of a multi-part task is incomplete (spec 9.5's
//! "task partially completed") instead of just refusing to advance.

use crate::lesson::Validator;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationRequest {
    pub lesson_id: String,
    pub task_id: String,
    pub validators: Vec<Validator>,
    /// Guest user whose interactive shell state (working directory, jobs) is inspected.
    #[serde(default = "default_subject")]
    pub subject_user: String,
    /// Lesson root inside the guest, used to resolve fixture and script references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson_root: Option<String>,
    /// Unix seconds when the attempt started; scopes journal queries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_started_at: Option<i64>,
}

fn default_subject() -> String {
    "student".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutcome {
    /// Validator tag, e.g. `directory_exists`.
    pub kind: String,
    pub passed: bool,
    /// Learner-facing explanation. Present whether or not the check passed.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<FailureCategory>,
    /// What the guest actually observed, for the Validation panel. Never raw command output
    /// that might leak hidden fixtures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(default = "default_weight")]
    pub weight: u32,
    /// Set when the validator could not run at all (unknown tag, bad params, timeout).
    /// An errored check never counts as passed.
    #[serde(default)]
    pub errored: bool,
}

fn default_weight() -> u32 {
    1
}

impl CheckOutcome {
    pub fn pass(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            passed: true,
            message: message.into(),
            failure_category: None,
            observed: None,
            expected: None,
            weight: 1,
            errored: false,
        }
    }

    pub fn fail(
        kind: impl Into<String>,
        message: impl Into<String>,
        category: FailureCategory,
    ) -> Self {
        Self {
            kind: kind.into(),
            passed: false,
            message: message.into(),
            failure_category: Some(category),
            observed: None,
            expected: None,
            weight: 1,
            errored: false,
        }
    }

    /// The validator itself was unusable. Reported distinctly from a learner mistake so a
    /// broken lesson package cannot masquerade as a wrong answer.
    pub fn error(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            passed: false,
            message: message.into(),
            failure_category: None,
            observed: None,
            expected: None,
            weight: 1,
            errored: true,
        }
    }

    pub fn observed(mut self, observed: impl Into<String>) -> Self {
        self.observed = Some(observed.into());
        self
    }

    pub fn expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn weighted(mut self, weight: u32) -> Self {
        self.weight = weight.max(1);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskValidation {
    pub lesson_id: String,
    pub task_id: String,
    pub passed: bool,
    pub outcomes: Vec<CheckOutcome>,
    /// 0-100, weighted by each check's `weight`. Drives the "task partially completed" message.
    pub completion_percent: u8,
    /// The single most useful failure to surface first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_failure: Option<CheckOutcome>,
    #[serde(default)]
    pub errored: bool,
}

impl TaskValidation {
    pub fn from_outcomes(
        lesson_id: impl Into<String>,
        task_id: impl Into<String>,
        outcomes: Vec<CheckOutcome>,
    ) -> Self {
        let total: u32 = outcomes.iter().map(|o| o.weight).sum();
        let earned: u32 = outcomes.iter().filter(|o| o.passed).map(|o| o.weight).sum();
        let completion_percent = if total == 0 {
            0
        } else {
            ((earned as f64 / total as f64) * 100.0).round() as u8
        };
        let passed = !outcomes.is_empty() && outcomes.iter().all(|o| o.passed);
        let errored = outcomes.iter().any(|o| o.errored);
        // Prefer a real failure over an errored check when both exist: the learner can act
        // on the former, and the latter is an authoring bug we also log separately.
        let primary_failure = outcomes
            .iter()
            .find(|o| !o.passed && !o.errored)
            .or_else(|| outcomes.iter().find(|o| !o.passed))
            .cloned();

        Self {
            lesson_id: lesson_id.into(),
            task_id: task_id.into(),
            passed,
            outcomes,
            completion_percent,
            primary_failure,
            errored,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSummary {
    pub lesson_id: String,
    pub tasks_passed: usize,
    pub tasks_total: usize,
    pub complete: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategory {
    CommandNotFound,
    InvalidOption,
    WrongWorkingDirectory,
    WrongPath,
    PermissionDenied,
    FileAlreadyExists,
    WrongFileType,
    WrongFileContents,
    WrongOwnership,
    WrongPermissions,
    ProcessNotRunning,
    ServiceNotActive,
    WrongPort,
    IncorrectRedirect,
    PipelineOutputIncorrect,
    ScriptSyntaxFailure,
    ScriptLogicFailure,
    NetworkUnreachable,
    DnsFailure,
    TaskPartiallyCompleted,
}

impl FailureCategory {
    /// Short guidance shown beside the failure in the Validation panel. Deliberately about
    /// the *class* of mistake; the validator supplies the specifics.
    pub fn guidance(self) -> &'static str {
        match self {
            Self::CommandNotFound => {
                "The shell could not find that command. Check the spelling, and check PATH."
            }
            Self::InvalidOption => {
                "The command ran but rejected an option. Compare it against --help."
            }
            Self::WrongWorkingDirectory => {
                "You are not in the directory the task expects. Run pwd to check."
            }
            Self::WrongPath => "The path does not exist yet, or it was created somewhere else.",
            Self::PermissionDenied => {
                "The account you used is not allowed to do that. Consider whether sudo is required."
            }
            Self::FileAlreadyExists => {
                "Something already exists at that path, so the command refused to overwrite it."
            }
            Self::WrongFileType => {
                "The path exists but is the wrong kind of object, for example a directory where a file was expected."
            }
            Self::WrongFileContents => "The file exists but its contents are not what the task asked for.",
            Self::WrongOwnership => "The owner or group is not what the task asked for.",
            Self::WrongPermissions => "The permission bits are not what the task asked for.",
            Self::ProcessNotRunning => "The expected process is not running.",
            Self::ServiceNotActive => "The service is not in the expected state. Check systemctl status.",
            Self::WrongPort => "Nothing is listening on the expected port, or it is bound to the wrong address.",
            Self::IncorrectRedirect => {
                "Output went somewhere other than where the task expected. Check > against >> and 2>."
            }
            Self::PipelineOutputIncorrect => {
                "The pipeline ran but produced different output. Compare it a stage at a time."
            }
            Self::ScriptSyntaxFailure => "The script does not parse. Check quoting and block keywords.",
            Self::ScriptLogicFailure => "The script runs but produces the wrong result.",
            Self::NetworkUnreachable => "The target could not be reached. Work up from the interface.",
            Self::DnsFailure => "The name did not resolve. Test the resolver directly with dig.",
            Self::TaskPartiallyCompleted => "Part of this task is done. The remaining checks are listed below.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_percent_is_weighted() {
        let outcomes = vec![
            CheckOutcome::pass("directory_exists", "ok").weighted(3),
            CheckOutcome::fail("file_exists", "missing", FailureCategory::WrongPath).weighted(1),
        ];
        let v = TaskValidation::from_outcomes("l", "task-1", outcomes);
        assert!(!v.passed);
        assert_eq!(v.completion_percent, 75);
        assert_eq!(v.primary_failure.unwrap().kind, "file_exists");
    }

    #[test]
    fn all_passing_marks_task_passed() {
        let v = TaskValidation::from_outcomes(
            "l",
            "task-1",
            vec![CheckOutcome::pass("file_exists", "ok")],
        );
        assert!(v.passed);
        assert_eq!(v.completion_percent, 100);
        assert!(v.primary_failure.is_none());
    }

    #[test]
    fn empty_validator_list_never_passes() {
        let v = TaskValidation::from_outcomes("l", "task-1", vec![]);
        assert!(!v.passed);
        assert_eq!(v.completion_percent, 0);
    }

    #[test]
    fn errored_check_is_not_a_pass_and_real_failures_are_preferred() {
        let outcomes = vec![
            CheckOutcome::error("mystery_validator", "unknown validator"),
            CheckOutcome::fail("file_exists", "missing", FailureCategory::WrongPath),
        ];
        let v = TaskValidation::from_outcomes("l", "task-1", outcomes);
        assert!(!v.passed);
        assert!(v.errored);
        assert_eq!(v.primary_failure.unwrap().kind, "file_exists");
    }
}
