//! Turns raw validation outcomes into the message the learner reads (spec 9.5).
//!
//! Two rules shape everything here. First, tell the learner *which* class of mistake this is,
//! because "wrong path" and "permission denied" call for completely different next steps.
//! Second, never reveal the answer: feedback describes the gap, it does not close it.

use shared_types::{FailureCategory, TaskValidation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    /// One-line headline.
    pub headline: String,
    /// The class of mistake, when the checks agreed on one.
    pub category: Option<FailureCategory>,
    /// Generic guidance for that class.
    pub guidance: Option<String>,
    /// Specific, per-check detail. Empty when the task passed.
    pub details: Vec<String>,
    /// True when some checks passed: worth saying so rather than a flat "incorrect".
    pub partial: bool,
    /// Set when the lesson package itself is broken. Surfaced differently in the UI, because
    /// it is not the learner's fault.
    pub authoring_error: bool,
}

pub fn shape_feedback(validation: &TaskValidation) -> Feedback {
    if validation.passed {
        return Feedback {
            headline: "Correct.".to_string(),
            category: None,
            guidance: None,
            details: vec![],
            partial: false,
            authoring_error: false,
        };
    }

    let failed: Vec<_> = validation.outcomes.iter().filter(|o| !o.passed).collect();
    let details: Vec<String> = failed
        .iter()
        .map(|o| {
            let mut line = o.message.clone();
            if let (Some(expected), Some(observed)) = (&o.expected, &o.observed) {
                line.push_str(&format!(" (expected {expected}, found {observed})"));
            } else if let Some(observed) = &o.observed {
                line.push_str(&format!(" (found {observed})"));
            }
            line
        })
        .collect();

    // A validator that could not run at all is an authoring bug, not a wrong answer, and
    // saying "incorrect" to the learner would be a lie.
    if validation.errored && failed.iter().all(|o| o.errored) {
        return Feedback {
            headline: "This task could not be checked.".to_string(),
            category: None,
            guidance: Some(
                "The lesson package is faulty rather than your answer. Reporting this with the \
                 diagnostic export helps get it fixed."
                    .to_string(),
            ),
            details,
            partial: false,
            authoring_error: true,
        };
    }

    let partial = validation.completion_percent > 0;
    // Prefer the category the checks agree on. When they disagree, the task is partly done
    // and the useful framing is progress, not a single cause.
    let categories: Vec<FailureCategory> =
        failed.iter().filter_map(|o| o.failure_category).collect();
    let unanimous = categories
        .first()
        .copied()
        .filter(|first| categories.iter().all(|c| c == first));

    let category = match (unanimous, partial) {
        (Some(single), _) => Some(single),
        (None, true) => Some(FailureCategory::TaskPartiallyCompleted),
        (None, false) => None,
    };

    let headline = if partial {
        format!(
            "Almost there: {}% of this task is done.",
            validation.completion_percent
        )
    } else {
        "Not quite yet.".to_string()
    };

    Feedback {
        headline,
        category,
        guidance: category.map(|c| c.guidance().to_string()),
        details,
        partial,
        authoring_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::CheckOutcome;

    fn validation(outcomes: Vec<CheckOutcome>) -> TaskValidation {
        TaskValidation::from_outcomes("l", "task-1", outcomes)
    }

    #[test]
    fn passing_says_so_and_nothing_more() {
        let f = shape_feedback(&validation(vec![CheckOutcome::pass("file_exists", "ok")]));
        assert_eq!(f.headline, "Correct.");
        assert!(f.details.is_empty());
        assert!(!f.partial);
    }

    #[test]
    fn a_single_cause_is_reported_with_its_guidance() {
        let f = shape_feedback(&validation(vec![CheckOutcome::fail(
            "directory_exists",
            "The directory /home/student/reports does not exist.",
            FailureCategory::WrongPath,
        )]));
        assert_eq!(f.category, Some(FailureCategory::WrongPath));
        assert!(f.guidance.unwrap().contains("does not exist yet"));
        assert!(!f.partial);
        assert_eq!(f.headline, "Not quite yet.");
    }

    #[test]
    fn partial_completion_leads_with_progress() {
        let f = shape_feedback(&validation(vec![
            CheckOutcome::pass("directory_exists", "ok"),
            CheckOutcome::fail("file_exists", "missing", FailureCategory::WrongPath),
        ]));
        assert!(f.partial);
        assert!(f.headline.contains("50%"), "{}", f.headline);
    }

    #[test]
    fn disagreeing_causes_become_partially_completed() {
        let f = shape_feedback(&validation(vec![
            CheckOutcome::pass("directory_exists", "ok"),
            CheckOutcome::fail("file_owner", "wrong owner", FailureCategory::WrongOwnership),
            CheckOutcome::fail("file_mode", "wrong mode", FailureCategory::WrongPermissions),
        ]));
        assert_eq!(f.category, Some(FailureCategory::TaskPartiallyCompleted));
    }

    #[test]
    fn observed_and_expected_are_folded_into_the_detail_line() {
        let outcome = CheckOutcome::fail(
            "file_mode",
            "The permissions are not what the task asked for.",
            FailureCategory::WrongPermissions,
        )
        .expected("0640")
        .observed("0644");
        let f = shape_feedback(&validation(vec![outcome]));
        assert_eq!(
            f.details[0],
            "The permissions are not what the task asked for. (expected 0640, found 0644)"
        );
    }

    #[test]
    fn a_broken_validator_is_never_reported_as_a_wrong_answer() {
        let f = shape_feedback(&validation(vec![CheckOutcome::error(
            "nonexistent_validator",
            "unknown validator 'nonexistent_validator'",
        )]));
        assert!(f.authoring_error);
        assert_eq!(f.headline, "This task could not be checked.");
        assert!(f
            .guidance
            .unwrap()
            .contains("faulty rather than your answer"));
    }

    #[test]
    fn a_real_failure_alongside_a_broken_validator_still_coaches_the_learner() {
        let f = shape_feedback(&validation(vec![
            CheckOutcome::error("broken", "unknown validator"),
            CheckOutcome::fail("file_exists", "missing", FailureCategory::WrongPath),
        ]));
        assert!(!f.authoring_error);
        // The actionable category wins; the broken validator is logged, not blamed on the learner.
        assert_eq!(f.category, Some(FailureCategory::WrongPath));
        assert_eq!(f.details.len(), 2);
    }
}
