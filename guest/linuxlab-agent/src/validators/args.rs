//! Parameter access helpers.
//!
//! Every validator has already been checked against the registry before it reaches here, so
//! a missing parameter at this point is a bug rather than bad input. These helpers still
//! return an *errored* outcome instead of panicking, because a panic in the agent would take
//! the whole guest session down and the learner would lose their terminal.

use shared_types::{CheckOutcome, FailureCategory, Validator};
use std::path::PathBuf;
use std::time::Duration;

pub type ArgResult<T> = Result<T, CheckOutcome>;

fn missing(validator: &Validator, param: &str) -> CheckOutcome {
    CheckOutcome::error(
        &validator.kind,
        format!(
            "internal error: validator '{}' reached the agent without its '{param}' parameter",
            validator.kind
        ),
    )
}

pub fn string<'a>(validator: &'a Validator, param: &str) -> ArgResult<&'a str> {
    validator
        .str_param(param)
        .ok_or_else(|| missing(validator, param))
}

pub fn path(validator: &Validator, param: &str) -> ArgResult<PathBuf> {
    Ok(PathBuf::from(string(validator, param)?))
}

pub fn flag(validator: &Validator, param: &str) -> bool {
    validator.bool_param(param).unwrap_or(false)
}

pub fn flag_or(validator: &Validator, param: &str, default: bool) -> bool {
    validator.bool_param(param).unwrap_or(default)
}

pub fn optional_string<'a>(validator: &'a Validator, param: &str) -> Option<&'a str> {
    validator.str_param(param)
}

pub fn optional_integer(validator: &Validator, param: &str) -> Option<i64> {
    validator.i64_param(param)
}

pub fn string_list(validator: &Validator, param: &str) -> ArgResult<Vec<String>> {
    let value = validator
        .params
        .get(param)
        .ok_or_else(|| missing(validator, param))?;
    let items = value
        .as_array()
        .ok_or_else(|| missing(validator, param))?
        .iter()
        .filter_map(|item| item.as_str().map(|s| s.to_string()))
        .collect();
    Ok(items)
}

pub fn timeout(validator: &Validator) -> Option<Duration> {
    validator
        .i64_param("timeoutMs")
        .filter(|ms| *ms > 0)
        // Capped so a lesson package cannot wedge the validation run indefinitely.
        .map(|ms| Duration::from_millis((ms as u64).min(120_000)))
}

/// Bounds shared by `file_size`, `file_line_count` and `process_count`.
pub struct Bounds {
    pub equals: Option<i64>,
    pub min: Option<i64>,
    pub max: Option<i64>,
}

impl Bounds {
    pub fn read(validator: &Validator) -> Self {
        Self {
            equals: validator.i64_param("equals"),
            min: validator.i64_param("min"),
            max: validator.i64_param("max"),
        }
    }

    pub fn satisfied_by(&self, actual: i64) -> bool {
        if let Some(equals) = self.equals {
            if actual != equals {
                return false;
            }
        }
        if let Some(min) = self.min {
            if actual < min {
                return false;
            }
        }
        if let Some(max) = self.max {
            if actual > max {
                return false;
            }
        }
        true
    }

    pub fn describe(&self) -> String {
        match (self.equals, self.min, self.max) {
            (Some(equals), _, _) => format!("exactly {equals}"),
            (None, Some(min), Some(max)) => format!("between {min} and {max}"),
            (None, Some(min), None) => format!("at least {min}"),
            (None, None, Some(max)) => format!("at most {max}"),
            (None, None, None) => "any value".to_string(),
        }
    }
}

/// Applies the author's `message`, `failureCategory` and `weight` overrides to an outcome.
/// Called once on the way out of dispatch so no individual validator has to remember to.
pub fn apply_overrides(validator: &Validator, mut outcome: CheckOutcome) -> CheckOutcome {
    if !outcome.passed && !outcome.errored {
        if let Some(message) = validator.str_param("message") {
            outcome.message = message.to_string();
        }
        if let Some(category) = validator
            .str_param("failureCategory")
            .and_then(parse_failure_category)
        {
            outcome.failure_category = Some(category);
        }
    }
    if let Some(weight) = validator.i64_param("weight") {
        outcome.weight = weight.clamp(1, 100) as u32;
    }
    outcome
}

fn parse_failure_category(value: &str) -> Option<FailureCategory> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_handle_each_combination() {
        let exact = Bounds {
            equals: Some(5),
            min: None,
            max: None,
        };
        assert!(exact.satisfied_by(5));
        assert!(!exact.satisfied_by(4));
        assert_eq!(exact.describe(), "exactly 5");

        let range = Bounds {
            equals: None,
            min: Some(2),
            max: Some(4),
        };
        assert!(range.satisfied_by(3));
        assert!(!range.satisfied_by(1));
        assert!(!range.satisfied_by(5));
        assert_eq!(range.describe(), "between 2 and 4");

        let lower = Bounds {
            equals: None,
            min: Some(1),
            max: None,
        };
        assert!(lower.satisfied_by(1_000));
        assert_eq!(lower.describe(), "at least 1");
    }

    #[test]
    fn author_message_replaces_a_generated_failure_message() {
        let validator = Validator::new("file_exists")
            .with("path", "/tmp/x")
            .with("message", "Create the report before checking.");
        let outcome = apply_overrides(
            &validator,
            CheckOutcome::fail("file_exists", "generated text", FailureCategory::WrongPath),
        );
        assert_eq!(outcome.message, "Create the report before checking.");
    }

    #[test]
    fn author_overrides_never_rewrite_a_pass_or_an_internal_error() {
        let validator = Validator::new("file_exists")
            .with("path", "/tmp/x")
            .with("message", "custom");

        let passed = apply_overrides(&validator, CheckOutcome::pass("file_exists", "ok"));
        assert_eq!(passed.message, "ok");

        let errored = apply_overrides(&validator, CheckOutcome::error("file_exists", "broken"));
        assert_eq!(
            errored.message, "broken",
            "authors must not mask agent bugs"
        );
    }

    #[test]
    fn failure_category_override_is_applied() {
        let validator = Validator::new("file_exists")
            .with("path", "/tmp/x")
            .with("failureCategory", "permission_denied");
        let outcome = apply_overrides(
            &validator,
            CheckOutcome::fail("file_exists", "m", FailureCategory::WrongPath),
        );
        assert_eq!(
            outcome.failure_category,
            Some(FailureCategory::PermissionDenied)
        );
    }

    #[test]
    fn weight_is_clamped_to_a_sane_range() {
        let heavy = Validator::new("file_exists")
            .with("path", "/tmp/x")
            .with("weight", 9_000);
        assert_eq!(
            apply_overrides(&heavy, CheckOutcome::pass("file_exists", "ok")).weight,
            100
        );
        let zero = Validator::new("file_exists")
            .with("path", "/tmp/x")
            .with("weight", 0);
        assert_eq!(
            apply_overrides(&zero, CheckOutcome::pass("file_exists", "ok")).weight,
            1
        );
    }

    #[test]
    fn timeouts_are_capped() {
        let long = Validator::new("stdout_contains")
            .with("command", "true")
            .with("text", "x")
            .with("timeoutMs", 10_000_000);
        assert_eq!(timeout(&long), Some(Duration::from_millis(120_000)));

        let none = Validator::new("stdout_contains")
            .with("command", "true")
            .with("text", "x");
        assert_eq!(timeout(&none), None);
    }

    #[test]
    fn a_missing_param_yields_an_errored_outcome_not_a_panic() {
        let validator = Validator::new("file_exists");
        let outcome = path(&validator, "path").unwrap_err();
        assert!(outcome.errored);
        assert!(!outcome.passed);
    }
}
