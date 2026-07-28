//! Lesson catalogue, progression rules, hint state and feedback shaping.
//!
//! This crate is pure logic: it never touches the guest, the database or the filesystem
//! beyond loading lesson packages. That is what lets the whole learning model be tested
//! without a virtual machine.

pub mod catalog;
pub mod feedback;
pub mod hints;
pub mod progression;

#[cfg(test)]
pub(crate) mod test_support;

pub use catalog::{Catalog, CatalogIssue};
pub use feedback::{shape_feedback, Feedback};
pub use hints::{lesson_mastery, AttemptState, HintOutcome, HintReveal};
pub use progression::{Availability, LessonState, ProgressIndex, Progression, ProgressionMode};

use anyhow::Result;
use shared_types::{Lesson, Task, ValidationRequest};

/// Builds the validation request for one task, resolving the lesson's environment.
pub fn build_validation_request(
    lesson: &Lesson,
    task: &Task,
    attempt_started_at: i64,
) -> ValidationRequest {
    ValidationRequest {
        lesson_id: lesson.id.clone(),
        task_id: task.id.clone(),
        validators: task.validators.clone(),
        subject_user: "student".to_string(),
        lesson_root: Some(format!("/opt/linuxlab/lessons/{}", lesson.id)),
        attempt_started_at: Some(attempt_started_at),
    }
}

/// Checks every validator in a lesson against the registry before it is sent to the guest.
/// A lesson package is untrusted input (spec 21.4), so this runs on load *and* per attempt.
pub fn check_task_validators(task: &Task) -> Result<()> {
    for validator in &task.validators {
        shared_types::registry()
            .check(validator)
            .map_err(|err| anyhow::anyhow!("task {} has an invalid validator: {err}", task.id))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{TaskKind, Validator};

    fn task(validators: Vec<Validator>) -> Task {
        Task {
            id: "task-1".into(),
            kind: TaskKind::Guided,
            instruction: "Do the thing that the lesson asks for.".into(),
            context: None,
            broken_command: None,
            diagnosis: None,
            validators,
            hints: vec![],
            suggested_solution: Some("true".into()),
            known_incorrect_solution: None,
            alternate_solutions: vec![],
            optional: false,
        }
    }

    #[test]
    fn valid_tasks_pass_the_registry_check() {
        let t = task(vec![
            Validator::new("directory_exists").with("path", "/home/student/reports")
        ]);
        assert!(check_task_validators(&t).is_ok());
    }

    #[test]
    fn invalid_tasks_are_rejected_with_the_task_id_in_the_message() {
        let t = task(vec![Validator::new("directory_exists")]);
        let err = check_task_validators(&t).unwrap_err().to_string();
        assert!(err.contains("task-1"), "{err}");
        assert!(err.contains("requires parameter 'path'"), "{err}");
    }
}
