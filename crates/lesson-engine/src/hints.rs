//! Hint progression and attempt bookkeeping (spec 9.4, 9.6).
//!
//! Hints are revealed one at a time, conceptual first. The solution sits behind the last
//! hint and is tracked separately, because revealing it changes the mastery outcome even
//! when no hints were opened. The learner always has to type the answer themselves: nothing
//! here writes to the terminal.

use serde::{Deserialize, Serialize};
use shared_types::{FailureCategory, MasteryStatus, Task};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AttemptState {
    pub lesson_id: String,
    pub task_id: String,
    /// How many hints the learner has opened for this task.
    pub hints_revealed: usize,
    pub solution_revealed: bool,
    pub failed_checks: u32,
    /// Distinct failure categories seen, for the "weak concepts" report.
    pub failure_categories: Vec<FailureCategory>,
    pub passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HintReveal {
    pub index: usize,
    pub text: String,
    pub remaining: usize,
    /// True when the next request would reveal the worked solution.
    pub solution_next: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintOutcome {
    Hint(HintReveal),
    /// All hints are used; the caller may offer "Show solution" explicitly.
    SolutionAvailable,
    /// Assessment lessons offer neither.
    Unavailable(&'static str),
}

impl AttemptState {
    pub fn new(lesson_id: impl Into<String>, task_id: impl Into<String>) -> Self {
        Self {
            lesson_id: lesson_id.into(),
            task_id: task_id.into(),
            ..Default::default()
        }
    }

    /// Reveals the next hint, if the lesson allows hints at all.
    pub fn reveal_next_hint(&mut self, task: &Task, hints_allowed: bool) -> HintOutcome {
        if !hints_allowed {
            return HintOutcome::Unavailable(
                "Hints are not available in an assessment. Work from what you know, and review \
                 the lesson afterwards.",
            );
        }
        if task.hints.is_empty() {
            return HintOutcome::SolutionAvailable;
        }
        if self.hints_revealed >= task.hints.len() {
            return HintOutcome::SolutionAvailable;
        }

        let index = self.hints_revealed;
        self.hints_revealed += 1;
        let remaining = task.hints.len() - self.hints_revealed;
        HintOutcome::Hint(HintReveal {
            index,
            text: task.hints[index].clone(),
            remaining,
            solution_next: remaining == 0,
        })
    }

    /// Marks the worked solution as shown. Idempotent so a double click does not double-count.
    pub fn reveal_solution(&mut self, hints_allowed: bool) -> Option<&'static str> {
        if !hints_allowed {
            return Some("The solution is not available in an assessment.");
        }
        self.solution_revealed = true;
        None
    }

    pub fn record_failure(&mut self, category: Option<FailureCategory>) {
        self.failed_checks += 1;
        if let Some(category) = category {
            if !self.failure_categories.contains(&category) {
                self.failure_categories.push(category);
            }
        }
    }

    pub fn record_pass(&mut self) {
        self.passed = true;
    }

    pub fn mastery(&self, is_assessment: bool) -> MasteryStatus {
        MasteryStatus::evaluate(
            self.passed,
            is_assessment,
            self.hints_revealed as u32,
            self.solution_revealed,
        )
    }
}

/// Rolls per-task attempts up into a lesson result. The lesson takes the *worst* mastery of
/// its tasks: a learner who needed the answer for one task has not mastered the lesson.
pub fn lesson_mastery(attempts: &[AttemptState], is_assessment: bool) -> MasteryStatus {
    if attempts.is_empty() {
        return MasteryStatus::NeedsReview;
    }
    attempts
        .iter()
        .map(|a| a.mastery(is_assessment))
        .min()
        .unwrap_or(MasteryStatus::NeedsReview)
}

/// Total hints opened across a lesson, stored on lesson_progress.
pub fn total_hints(attempts: &[AttemptState]) -> u32 {
    attempts.iter().map(|a| a.hints_revealed as u32).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{TaskKind, Validator};

    fn task_with_hints(hints: &[&str]) -> Task {
        Task {
            id: "task-1".into(),
            kind: TaskKind::Guided,
            instruction: "Create the reports directory.".into(),
            context: None,
            broken_command: None,
            diagnosis: None,
            validators: vec![
                Validator::new("directory_exists").with("path", "/home/student/reports")
            ],
            hints: hints.iter().map(|h| h.to_string()).collect(),
            suggested_solution: Some("mkdir reports".into()),
            known_incorrect_solution: None,
            alternate_solutions: vec![],
            optional: false,
        }
    }

    #[test]
    fn hints_are_revealed_one_at_a_time_in_order() {
        let task = task_with_hints(&["conceptual", "the command is mkdir", "mkdir NAME"]);
        let mut state = AttemptState::new("l", "task-1");

        let HintOutcome::Hint(first) = state.reveal_next_hint(&task, true) else {
            panic!("expected a hint");
        };
        assert_eq!(first.index, 0);
        assert_eq!(first.text, "conceptual");
        assert_eq!(first.remaining, 2);
        assert!(!first.solution_next);

        state.reveal_next_hint(&task, true);
        let HintOutcome::Hint(third) = state.reveal_next_hint(&task, true) else {
            panic!("expected a hint");
        };
        assert!(third.solution_next);
        assert_eq!(state.hints_revealed, 3);
    }

    #[test]
    fn running_out_of_hints_offers_the_solution_without_revealing_it() {
        let task = task_with_hints(&["only one"]);
        let mut state = AttemptState::new("l", "task-1");
        state.reveal_next_hint(&task, true);
        assert_eq!(
            state.reveal_next_hint(&task, true),
            HintOutcome::SolutionAvailable
        );
        // Merely running out must not count as revealing the answer.
        assert!(!state.solution_revealed);
        assert_eq!(state.hints_revealed, 1);
    }

    #[test]
    fn assessments_offer_neither_hints_nor_the_solution() {
        let task = task_with_hints(&["a hint"]);
        let mut state = AttemptState::new("l", "task-1");
        assert!(matches!(
            state.reveal_next_hint(&task, false),
            HintOutcome::Unavailable(_)
        ));
        assert_eq!(state.hints_revealed, 0);
        assert!(state.reveal_solution(false).is_some());
        assert!(!state.solution_revealed);
    }

    #[test]
    fn revealing_the_solution_twice_counts_once() {
        let mut state = AttemptState::new("l", "task-1");
        assert!(state.reveal_solution(true).is_none());
        assert!(state.reveal_solution(true).is_none());
        assert!(state.solution_revealed);
    }

    #[test]
    fn mastery_reflects_hint_usage() {
        let task = task_with_hints(&["a", "b"]);

        let mut clean = AttemptState::new("l", "task-1");
        clean.record_pass();
        assert_eq!(clean.mastery(false), MasteryStatus::Mastered);

        let mut one_hint = AttemptState::new("l", "task-1");
        one_hint.reveal_next_hint(&task, true);
        one_hint.record_pass();
        assert_eq!(one_hint.mastery(false), MasteryStatus::Strong);

        let mut two_hints = AttemptState::new("l", "task-1");
        two_hints.reveal_next_hint(&task, true);
        two_hints.reveal_next_hint(&task, true);
        two_hints.record_pass();
        assert_eq!(two_hints.mastery(false), MasteryStatus::Passed);
    }

    #[test]
    fn lesson_mastery_takes_the_weakest_task() {
        let mut mastered = AttemptState::new("l", "task-1");
        mastered.record_pass();
        let mut struggled = AttemptState::new("l", "task-2");
        struggled.reveal_solution(true);
        struggled.record_pass();

        assert_eq!(
            lesson_mastery(&[mastered, struggled], false),
            MasteryStatus::NeedsReview
        );
    }

    #[test]
    fn failure_categories_are_deduplicated() {
        let mut state = AttemptState::new("l", "task-1");
        state.record_failure(Some(FailureCategory::WrongPath));
        state.record_failure(Some(FailureCategory::WrongPath));
        state.record_failure(Some(FailureCategory::PermissionDenied));
        assert_eq!(state.failed_checks, 3);
        assert_eq!(state.failure_categories.len(), 2);
    }
}
