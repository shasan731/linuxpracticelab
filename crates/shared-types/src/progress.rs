//! Progress and mastery types (spec 9.6, 17, 18). All of this stays on the local machine.

use crate::validation::FailureCategory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum LessonStatus {
    NotStarted,
    InProgress,
    Passed,
    NeedsReview,
}

/// The mastery ladder from spec 9.6. `Passed` and `NeedsReview` both still let the learner
/// move on: hints must never block progression.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum MasteryStatus {
    ReviewRequired,
    NeedsReview,
    Passed,
    Strong,
    Mastered,
}

impl MasteryStatus {
    /// Maps an attempt onto the ladder.
    ///
    /// `solution_revealed` outranks hint counting: looking at the answer means the lesson
    /// should come back around regardless of how few hints were opened first.
    pub fn evaluate(
        passed: bool,
        is_assessment: bool,
        hints_used: u32,
        solution_revealed: bool,
    ) -> Self {
        if !passed {
            return if is_assessment {
                MasteryStatus::ReviewRequired
            } else {
                MasteryStatus::NeedsReview
            };
        }
        if solution_revealed {
            return MasteryStatus::NeedsReview;
        }
        match hints_used {
            0 => MasteryStatus::Mastered,
            1 => MasteryStatus::Strong,
            _ => MasteryStatus::Passed,
        }
    }

    /// Weight used when averaging a module into a mastery percentage.
    pub fn score(self) -> u8 {
        match self {
            MasteryStatus::Mastered => 100,
            MasteryStatus::Strong => 85,
            MasteryStatus::Passed => 70,
            MasteryStatus::NeedsReview => 40,
            MasteryStatus::ReviewRequired => 0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MasteryStatus::Mastered => "Mastered",
            MasteryStatus::Strong => "Strong",
            MasteryStatus::Passed => "Passed",
            MasteryStatus::NeedsReview => "Needs review",
            MasteryStatus::ReviewRequired => "Review required",
        }
    }

    /// Lessons at or below this level are what "Review Weak Commands" offers first.
    pub fn needs_revisiting(self) -> bool {
        self <= MasteryStatus::NeedsReview
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LessonProgress {
    pub profile_id: i64,
    pub lesson_id: String,
    pub status: LessonStatus,
    pub attempts: u32,
    pub hints_used: u32,
    #[serde(default)]
    pub solution_revealed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mastery: Option<MasteryStatus>,
    pub mastery_score: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskAttempt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub profile_id: i64,
    pub lesson_id: String,
    pub task_id: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_category: Option<FailureCategory>,
    pub hints_used: u32,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModuleProgress {
    pub profile_id: i64,
    pub module_id: String,
    pub completed_lessons: u32,
    pub total_lessons: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment_score: Option<u8>,
    /// Average mastery across attempted lessons, 0-100.
    #[serde(default)]
    pub mastery_percent: u8,
}

impl ModuleProgress {
    pub fn completion_percent(&self) -> u8 {
        if self.total_lessons == 0 {
            return 0;
        }
        ((self.completed_lessons as f64 / self.total_lessons as f64) * 100.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mastery_ladder_matches_spec_table() {
        assert_eq!(
            MasteryStatus::evaluate(true, false, 0, false),
            MasteryStatus::Mastered
        );
        assert_eq!(
            MasteryStatus::evaluate(true, false, 1, false),
            MasteryStatus::Strong
        );
        assert_eq!(
            MasteryStatus::evaluate(true, false, 3, false),
            MasteryStatus::Passed
        );
        assert_eq!(
            MasteryStatus::evaluate(true, false, 0, true),
            MasteryStatus::NeedsReview
        );
        assert_eq!(
            MasteryStatus::evaluate(false, true, 0, false),
            MasteryStatus::ReviewRequired
        );
    }

    #[test]
    fn revealing_the_solution_outranks_a_low_hint_count() {
        // Zero hints but the answer was shown: still needs review.
        assert_eq!(
            MasteryStatus::evaluate(true, false, 0, true),
            MasteryStatus::NeedsReview
        );
    }

    #[test]
    fn weak_lessons_are_the_ones_at_or_below_needs_review() {
        assert!(MasteryStatus::NeedsReview.needs_revisiting());
        assert!(MasteryStatus::ReviewRequired.needs_revisiting());
        assert!(!MasteryStatus::Passed.needs_revisiting());
    }

    #[test]
    fn module_completion_rounds() {
        let p = ModuleProgress {
            profile_id: 1,
            module_id: "m".into(),
            completed_lessons: 1,
            total_lessons: 3,
            assessment_score: None,
            mastery_percent: 0,
        };
        assert_eq!(p.completion_percent(), 33);
    }
}
