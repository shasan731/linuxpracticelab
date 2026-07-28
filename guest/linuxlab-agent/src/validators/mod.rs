//! Validator dispatch.
//!
//! Every validator is checked against the embedded registry before it runs, and an unknown or
//! unimplemented tag produces an *errored* outcome rather than a pass. That distinction is the
//! whole safety property of this module: a lesson package that names a validator this agent
//! does not have must never be able to mark a task complete.

pub mod args;
pub mod filesystem;
pub mod identity;
pub mod network;
pub mod package;
pub mod process;
pub mod script;
pub mod service;

use shared_types::{CheckOutcome, TaskValidation, Validator};
use std::path::PathBuf;

/// Per-request context shared by every validator.
#[derive(Debug, Clone)]
pub struct Ctx {
    /// The learner whose shell state and permissions are inspected.
    pub subject_user: String,
    /// Unix seconds when this attempt began; scopes journal queries.
    pub attempt_started_at: Option<i64>,
    /// Namespace applied to network validators that do not name their own.
    pub default_namespace: Option<String>,
    lesson_root: PathBuf,
}

impl Ctx {
    pub fn new(subject_user: impl Into<String>, lesson_id: impl Into<String>) -> Self {
        let lesson_id = lesson_id.into();
        Self {
            subject_user: subject_user.into(),
            lesson_root: lesson_root_for(&lesson_id),
            attempt_started_at: None,
            default_namespace: None,
        }
    }

    #[cfg(test)]
    pub fn for_test(subject_user: &str) -> Self {
        Self::new(subject_user, "test.lesson.01")
    }

    pub fn lesson_root(&self) -> &std::path::Path {
        &self.lesson_root
    }

    /// A validator's own `namespace` wins over the lesson-wide default.
    pub fn namespace_for(&self, validator: &Validator) -> Option<String> {
        validator
            .str_param("namespace")
            .map(|s| s.to_string())
            .or_else(|| self.default_namespace.clone())
    }

    /// `journalctl --since` expression covering this attempt only.
    pub fn attempt_since_expression(&self) -> String {
        match self.attempt_started_at {
            // journalctl accepts an @-prefixed Unix timestamp.
            Some(seconds) => format!("@{seconds}"),
            None => "-10min".to_string(),
        }
    }
}

/// Lesson assets live under a fixed root, with the id sanitised so a crafted package cannot
/// point fixture lookups at arbitrary paths.
fn lesson_root_for(lesson_id: &str) -> PathBuf {
    let safe: String = lesson_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let safe = safe.trim_matches('.').to_string();
    PathBuf::from("/opt/linuxlab/lessons").join(if safe.is_empty() {
        "unknown".to_string()
    } else {
        safe
    })
}

/// Runs one validator: registry check, dispatch, then author overrides.
pub async fn evaluate(ctx: &Ctx, validator: &Validator) -> CheckOutcome {
    if let Err(err) = shared_types::registry().check(validator) {
        return CheckOutcome::error(
            &validator.kind,
            format!("this task cannot be checked: {err}"),
        );
    }

    let outcome = match dispatch(ctx, validator).await {
        Some(outcome) => outcome,
        // Reachable only if the registry says a validator is implemented but no module claims
        // it. The coverage test below exists to make that impossible in a released build.
        None => CheckOutcome::error(
            &validator.kind,
            format!(
                "validator '{}' is declared as implemented but this agent has no handler for it",
                validator.kind
            ),
        ),
    };

    args::apply_overrides(validator, outcome)
}

/// Tries each category module in turn. Modules return `None` for tags they do not own.
async fn dispatch(ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    if let Some(outcome) = filesystem::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    if let Some(outcome) = process::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    if let Some(outcome) = service::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    if let Some(outcome) = identity::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    if let Some(outcome) = network::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    if let Some(outcome) = script::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    if let Some(outcome) = package::dispatch(ctx, validator).await {
        return Some(outcome);
    }
    None
}

/// Runs every validator for a task and assembles the result.
pub async fn validate_task(
    ctx: &Ctx,
    lesson_id: &str,
    task_id: &str,
    validators: &[Validator],
) -> TaskValidation {
    let mut outcomes = Vec::with_capacity(validators.len());
    for validator in validators {
        outcomes.push(evaluate(ctx, validator).await);
    }
    TaskValidation::from_outcomes(lesson_id, task_id, outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::registry;

    /// Builds a validator carrying a plausible value for every required parameter, derived
    /// from the registry itself. Used only to prove a handler exists.
    fn probe(kind: &str) -> Validator {
        let spec = registry().spec(kind).expect("validator in registry");
        let mut validator = Validator::new(kind);
        for (name, param) in &spec.params {
            let required = param.required || spec.requires_one_of.first() == Some(name);
            if !required {
                continue;
            }
            let value: serde_json::Value = match param.param_type.as_str() {
                "int" => serde_json::json!(1),
                "port" => serde_json::json!(1),
                "bool" => serde_json::json!(true),
                "list<string>" => serde_json::json!(["probe"]),
                "validators" => {
                    serde_json::json!([{ "type": "file_exists", "path": "/nonexistent-probe" }])
                }
                "enum" => serde_json::json!(param.values.first().cloned().unwrap_or_default()),
                "mode" => serde_json::json!("0644"),
                "sha256" => serde_json::json!("0".repeat(64)),
                // `true` is a real command that exits immediately, so a probe cannot hang or
                // change the system.
                "command" => serde_json::json!("true"),
                "path" => serde_json::json!("/nonexistent-probe"),
                "user" => serde_json::json!("root"),
                "group" => serde_json::json!("root"),
                "unit" => serde_json::json!("linuxlab-probe.service"),
                "regex" => serde_json::json!("linuxlab-probe-pattern"),
                _ => serde_json::json!("probe"),
            };
            validator.params.insert(name.clone(), value);
        }
        // Keep every probe fast even where the guest tooling is absent.
        if spec.params.contains_key("timeoutMs") {
            validator
                .params
                .insert("timeoutMs".into(), serde_json::json!(1500));
        }
        validator
    }

    #[tokio::test]
    async fn every_validator_declared_implemented_has_a_handler() {
        // This is the test that stops a validator being added to validators.json and shipped
        // with nothing behind it, which would silently pass learners' tasks.
        let ctx = Ctx::for_test("root");
        let mut unwired = Vec::new();

        for kind in registry().implemented_validators() {
            let validator = probe(kind);
            assert!(
                registry().check(&validator).is_ok(),
                "probe for {kind} does not satisfy its own registry spec: {:?}",
                registry().check(&validator)
            );
            if dispatch(&ctx, &validator).await.is_none() {
                unwired.push(kind.clone());
            }
        }

        assert!(
            unwired.is_empty(),
            "validators declared implemented but not dispatched: {unwired:?}"
        );
    }

    #[tokio::test]
    async fn an_unknown_validator_errors_and_never_passes() {
        let ctx = Ctx::for_test("root");
        let outcome = evaluate(
            &ctx,
            &Validator::new("not_a_real_validator").with("path", "/tmp"),
        )
        .await;
        assert!(outcome.errored);
        assert!(!outcome.passed);
        assert!(
            outcome.message.contains("cannot be checked"),
            "{}",
            outcome.message
        );
    }

    #[tokio::test]
    async fn a_validator_missing_a_required_param_errors_rather_than_passing() {
        let ctx = Ctx::for_test("root");
        let outcome = evaluate(&ctx, &Validator::new("file_exists")).await;
        assert!(outcome.errored);
        assert!(!outcome.passed);
    }

    #[tokio::test]
    async fn a_task_with_one_broken_validator_does_not_pass() {
        let ctx = Ctx::for_test("root");
        let validation = validate_task(
            &ctx,
            "l",
            "task-1",
            &[
                Validator::new("directory_exists").with("path", "/"),
                Validator::new("nonexistent_validator"),
            ],
        )
        .await;
        assert!(!validation.passed);
        assert!(validation.errored);
    }

    #[tokio::test]
    async fn a_task_whose_checks_all_hold_passes() {
        let ctx = Ctx::for_test("root");
        let validation = validate_task(
            &ctx,
            "l",
            "task-1",
            &[
                Validator::new("directory_exists").with("path", "/"),
                Validator::new("file_exists").with("path", "/etc/passwd"),
            ],
        )
        .await;
        assert!(validation.passed, "{:?}", validation.outcomes);
        assert_eq!(validation.completion_percent, 100);
    }

    #[test]
    fn lesson_roots_cannot_escape_the_lessons_directory() {
        let root = lesson_root_for("../../etc/shadow");
        assert!(!root.to_string_lossy().contains(".."));
        assert!(root.starts_with("/opt/linuxlab/lessons"));

        let empty = lesson_root_for("...");
        assert_eq!(empty, PathBuf::from("/opt/linuxlab/lessons/unknown"));
    }

    #[test]
    fn a_validators_own_namespace_beats_the_lesson_default() {
        let mut ctx = Ctx::for_test("student");
        ctx.default_namespace = Some("student".into());
        let plain = Validator::new("interface_exists").with("interface", "eth0");
        assert_eq!(ctx.namespace_for(&plain).as_deref(), Some("student"));

        let explicit = plain.clone().with("namespace", "web1");
        assert_eq!(ctx.namespace_for(&explicit).as_deref(), Some("web1"));
    }

    #[test]
    fn journal_queries_are_scoped_to_the_current_attempt() {
        let mut ctx = Ctx::for_test("student");
        assert_eq!(ctx.attempt_since_expression(), "-10min");
        ctx.attempt_started_at = Some(1_700_000_000);
        assert_eq!(ctx.attempt_since_expression(), "@1700000000");
    }
}
