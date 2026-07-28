//! Progression rules (spec 9.7).
//!
//! Three modes coexist. Guided Path unlocks sequentially; Open Library never blocks access
//! but warns; Assessment Mode strips hints and examples. The important invariant is that
//! nothing here can make a lesson permanently unreachable — a learner who gets stuck can
//! always switch to Open Library and carry on.

use crate::catalog::Catalog;
use serde::{Deserialize, Serialize};
use shared_types::{LessonStatus, MasteryStatus};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressionMode {
    #[default]
    GuidedPath,
    OpenLibrary,
    Assessment,
}

/// What the learner has done so far, indexed by lesson id.
pub type ProgressIndex = HashMap<String, LessonState>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LessonState {
    pub status: LessonStatus,
    pub mastery: Option<MasteryStatus>,
}

impl LessonState {
    pub fn passed(mastery: MasteryStatus) -> Self {
        Self {
            status: LessonStatus::Passed,
            mastery: Some(mastery),
        }
    }

    fn is_passed(&self) -> bool {
        matches!(self.status, LessonStatus::Passed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Availability {
    pub lesson_id: String,
    /// Whether the lesson can be opened at all.
    pub unlocked: bool,
    /// Prerequisites that are not yet passed. Shown as a warning in Open Library and as the
    /// reason for the lock in Guided Path.
    pub missing_prerequisites: Vec<String>,
}

impl Availability {
    pub fn warning(&self) -> Option<String> {
        if self.missing_prerequisites.is_empty() {
            return None;
        }
        Some(format!(
            "This lesson builds on {}. You can still open it, but it may assume things you have not covered.",
            self.missing_prerequisites.join(", ")
        ))
    }
}

pub struct Progression<'a> {
    catalog: &'a Catalog,
    mode: ProgressionMode,
}

impl<'a> Progression<'a> {
    pub fn new(catalog: &'a Catalog, mode: ProgressionMode) -> Self {
        Self { catalog, mode }
    }

    pub fn availability(&self, lesson_id: &str, progress: &ProgressIndex) -> Availability {
        let missing = self
            .catalog
            .lesson(lesson_id)
            .map(|lesson| {
                lesson
                    .prerequisites
                    .iter()
                    .filter(|p| !progress.get(*p).map(|s| s.is_passed()).unwrap_or(false))
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let unlocked = match self.mode {
            // Open Library and Assessment Mode show a warning but never prevent access.
            ProgressionMode::OpenLibrary | ProgressionMode::Assessment => true,
            ProgressionMode::GuidedPath => missing.is_empty(),
        };

        Availability {
            lesson_id: lesson_id.to_string(),
            unlocked,
            missing_prerequisites: missing,
        }
    }

    /// The lesson "Continue" should open: the first core lesson that is not yet passed.
    pub fn next_lesson(&self, progress: &ProgressIndex) -> Option<&'a str> {
        self.catalog
            .core_lesson_ids()
            .iter()
            .find(|id| {
                !progress
                    .get(id.as_str())
                    .map(|s| s.is_passed())
                    .unwrap_or(false)
            })
            .map(|s| s.as_str())
    }

    /// Lessons worth revisiting, weakest first. Backs "Review Weak Commands".
    pub fn review_recommendations(&self, progress: &ProgressIndex, limit: usize) -> Vec<&'a str> {
        let mut weak: Vec<(&str, MasteryStatus)> = self
            .catalog
            .core_lesson_ids()
            .iter()
            .filter_map(|id| {
                let state = progress.get(id.as_str())?;
                let mastery = state.mastery?;
                mastery.needs_revisiting().then_some((id.as_str(), mastery))
            })
            .collect();
        // Weakest first, then curriculum order, which is already the iteration order.
        weak.sort_by_key(|(_, mastery)| *mastery);
        weak.into_iter().take(limit).map(|(id, _)| id).collect()
    }

    /// Count of passed core lessons, for the Home screen counter.
    pub fn completed_core_lessons(&self, progress: &ProgressIndex) -> usize {
        self.catalog
            .core_lesson_ids()
            .iter()
            .filter(|id| {
                progress
                    .get(id.as_str())
                    .map(|s| s.is_passed())
                    .unwrap_or(false)
            })
            .count()
    }

    /// Average mastery across attempted core lessons, 0-100.
    pub fn mastery_percent(&self, progress: &ProgressIndex) -> u8 {
        let scores: Vec<u8> = self
            .catalog
            .core_lesson_ids()
            .iter()
            .filter_map(|id| progress.get(id.as_str())?.mastery.map(|m| m.score()))
            .collect();
        if scores.is_empty() {
            return 0;
        }
        let total: u32 = scores.iter().map(|s| *s as u32).sum();
        (total / scores.len() as u32) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{write_lesson, write_module};
    use tempfile::TempDir;

    fn chain() -> (TempDir, Catalog) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_module(root, "core", "m", 0, &["m.01", "m.02", "m.03"]);
        write_lesson(root, "core", "m", "m.01", &[]);
        write_lesson(root, "core", "m", "m.02", &["m.01"]);
        write_lesson(root, "core", "m", "m.03", &["m.02"]);
        let catalog = Catalog::load(root).unwrap();
        (dir, catalog)
    }

    #[test]
    fn guided_path_locks_a_lesson_whose_prerequisite_is_unfinished() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let progress = ProgressIndex::new();

        assert!(p.availability("m.01", &progress).unlocked);
        let second = p.availability("m.02", &progress);
        assert!(!second.unlocked);
        assert_eq!(second.missing_prerequisites, vec!["m.01"]);
    }

    #[test]
    fn open_library_warns_but_never_blocks() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::OpenLibrary);
        let availability = p.availability("m.03", &ProgressIndex::new());
        assert!(availability.unlocked);
        assert!(availability
            .warning()
            .unwrap()
            .contains("You can still open it"));
    }

    #[test]
    fn passing_a_prerequisite_unlocks_the_next_lesson() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let mut progress = ProgressIndex::new();
        progress.insert("m.01".into(), LessonState::passed(MasteryStatus::Mastered));
        assert!(p.availability("m.02", &progress).unlocked);
        assert!(!p.availability("m.03", &progress).unlocked);
    }

    #[test]
    fn a_lesson_needing_review_still_counts_as_passed_for_unlocking() {
        // Hints must never prevent course progression (spec 9.6).
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let mut progress = ProgressIndex::new();
        progress.insert(
            "m.01".into(),
            LessonState::passed(MasteryStatus::NeedsReview),
        );
        assert!(p.availability("m.02", &progress).unlocked);
    }

    #[test]
    fn next_lesson_is_the_first_unfinished_one() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let mut progress = ProgressIndex::new();
        assert_eq!(p.next_lesson(&progress), Some("m.01"));
        progress.insert("m.01".into(), LessonState::passed(MasteryStatus::Strong));
        assert_eq!(p.next_lesson(&progress), Some("m.02"));
    }

    #[test]
    fn next_lesson_is_none_once_the_course_is_finished() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let mut progress = ProgressIndex::new();
        for id in ["m.01", "m.02", "m.03"] {
            progress.insert(id.into(), LessonState::passed(MasteryStatus::Mastered));
        }
        assert_eq!(p.next_lesson(&progress), None);
    }

    #[test]
    fn review_recommendations_put_the_weakest_first() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let mut progress = ProgressIndex::new();
        progress.insert(
            "m.01".into(),
            LessonState::passed(MasteryStatus::NeedsReview),
        );
        progress.insert(
            "m.02".into(),
            LessonState {
                status: LessonStatus::NeedsReview,
                mastery: Some(MasteryStatus::ReviewRequired),
            },
        );
        progress.insert("m.03".into(), LessonState::passed(MasteryStatus::Mastered));

        assert_eq!(p.review_recommendations(&progress, 5), vec!["m.02", "m.01"]);
    }

    #[test]
    fn mastery_percent_averages_only_attempted_lessons() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        let mut progress = ProgressIndex::new();
        progress.insert("m.01".into(), LessonState::passed(MasteryStatus::Mastered)); // 100
        progress.insert("m.02".into(), LessonState::passed(MasteryStatus::Passed)); // 70
        assert_eq!(p.mastery_percent(&progress), 85);
        assert_eq!(p.completed_core_lessons(&progress), 2);
    }

    #[test]
    fn mastery_percent_is_zero_before_anything_is_attempted() {
        let (_d, catalog) = chain();
        let p = Progression::new(&catalog, ProgressionMode::GuidedPath);
        assert_eq!(p.mastery_percent(&ProgressIndex::new()), 0);
    }
}
