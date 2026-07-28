//! Data transfer objects for the frontend.
//!
//! The important rule here is that suggested solutions, alternate solutions and review-question
//! answers are *not* sent to the webview with the lesson. They are fetched through explicit
//! commands that record the fact, so "the learner must still enter the solution manually"
//! (spec 9.4) cannot be sidestepped by opening developer tools and reading the lesson payload.

use lesson_engine::{Availability, Catalog, Feedback, ProgressionMode};
use serde::{Deserialize, Serialize};
use shared_types::{
    Demonstration, FailureCategory, Lesson, LessonEnvironment, LessonLevel, LessonProgress,
    LessonSummary, LessonType, MasteryStatus, Module, ReviewQuestionType, TaskKind, TaskValidation,
    VmStatus,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonView {
    pub id: String,
    pub title: String,
    pub level: LessonLevel,
    pub module: String,
    #[serde(rename = "type")]
    pub lesson_type: LessonType,
    pub estimated_difficulty: u8,
    pub estimated_minutes: Option<u32>,
    pub prerequisites: Vec<String>,
    pub concepts: Vec<String>,
    pub commands: Vec<String>,
    pub environment: LessonEnvironment,
    pub purpose: String,
    pub mental_model: String,
    pub syntax: Vec<String>,
    pub demonstration: Vec<Demonstration>,
    pub explanation_markdown: Option<String>,
    pub summary: Option<LessonSummary>,
    pub tasks: Vec<TaskView>,
    pub review_questions: Vec<ReviewQuestionView>,
    /// Whether hints and worked solutions are offered at all.
    pub hints_available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    pub id: String,
    pub kind: TaskKind,
    pub instruction: String,
    pub context: Option<String>,
    pub broken_command: Option<String>,
    /// How many hints exist. The text arrives one at a time from `reveal_hint`.
    pub hint_count: usize,
    pub optional: bool,
    /// What the task checks, in words. Lets the learner see the requirements without seeing
    /// the answer, which is what the Requirements panel shows.
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQuestionView {
    pub index: usize,
    #[serde(rename = "type")]
    pub question_type: ReviewQuestionType,
    pub question: String,
    pub answers: Vec<String>,
}

impl LessonView {
    pub fn from_lesson(lesson: &Lesson, mode: ProgressionMode) -> Self {
        let hints_available =
            !lesson.lesson_type.hides_hints() && mode != ProgressionMode::Assessment;

        Self {
            id: lesson.id.clone(),
            title: lesson.title.clone(),
            level: lesson.level,
            module: lesson.module.clone(),
            lesson_type: lesson.lesson_type,
            estimated_difficulty: lesson.estimated_difficulty,
            estimated_minutes: lesson.estimated_minutes,
            prerequisites: lesson.prerequisites.clone(),
            concepts: lesson.concepts.clone(),
            commands: lesson.commands.clone(),
            environment: lesson.environment.clone(),
            purpose: lesson.content.purpose.clone(),
            mental_model: lesson.content.mental_model.clone(),
            // Assessment mode strips worked examples as well as hints (spec 9.7).
            syntax: if hints_available {
                lesson.content.syntax.clone()
            } else {
                vec![]
            },
            demonstration: if hints_available {
                lesson.content.demonstration.clone()
            } else {
                vec![]
            },
            explanation_markdown: lesson.content.explanation_markdown.clone(),
            summary: lesson.content.summary.clone(),
            tasks: lesson
                .tasks
                .iter()
                .map(|task| TaskView {
                    id: task.id.clone(),
                    kind: task.kind,
                    instruction: task.instruction.clone(),
                    context: task.context.clone(),
                    broken_command: task.broken_command.clone(),
                    hint_count: if hints_available { task.hints.len() } else { 0 },
                    optional: task.optional,
                    requirements: task
                        .validators
                        .iter()
                        .filter_map(describe_validator)
                        .collect(),
                })
                .collect(),
            review_questions: lesson
                .review_questions
                .iter()
                .enumerate()
                .map(|(index, question)| ReviewQuestionView {
                    index,
                    question_type: question.question_type,
                    question: question.question.clone(),
                    answers: question.answers.clone(),
                })
                .collect(),
            hints_available,
        }
    }
}

/// Turns a validator into a requirement a learner can read.
///
/// Deliberately describes the *goal*, never the command: "the directory /home/student/reports
/// exists" tells you what to achieve, `mkdir reports` tells you what to type.
fn describe_validator(validator: &shared_types::Validator) -> Option<String> {
    let path = validator.str_param("path").unwrap_or_default();
    Some(match validator.kind.as_str() {
        "file_exists" => format!("The file {path} exists"),
        "file_missing" => format!("{path} no longer exists"),
        "directory_exists" => format!("The directory {path} exists"),
        "directory_missing" => format!("The directory {path} no longer exists"),
        "symbolic_link_exists" => format!("{path} is a symbolic link"),
        "hard_link_exists" => format!(
            "{path} and {} are hard links to the same file",
            validator.str_param("linkTo").unwrap_or_default()
        ),
        "file_type" => format!(
            "{path} is a {}",
            validator.str_param("fileType").unwrap_or_default()
        ),
        "file_owner" => format!(
            "{path} is owned by {}",
            validator.str_param("owner").unwrap_or_default()
        ),
        "file_group" => format!(
            "{path} belongs to the group {}",
            validator.str_param("group").unwrap_or_default()
        ),
        "file_mode" => format!(
            "{path} has the permissions {}",
            validator.str_param("mode").unwrap_or_default()
        ),
        "file_contains" => format!("{path} contains the required text"),
        "file_matches_regex" => format!("{path} matches the required pattern"),
        "file_line_count" => format!("{path} has the required number of lines"),
        "file_size" => format!("{path} is the required size"),
        "file_checksum" => format!("{path} has exactly the required contents"),
        "directory_contains" => format!("{path} contains the required entries"),
        "directory_empty" => format!("{path} is empty"),
        "current_directory" => format!("Your shell's working directory is {path}"),
        "process_running" => "The required process is running".to_string(),
        "process_not_running" => "The process is no longer running".to_string(),
        "process_owner" => format!(
            "The process runs as {}",
            validator.str_param("owner").unwrap_or_default()
        ),
        "background_job_running" => "The job is running in the background".to_string(),
        "process_signal_received" => format!(
            "The process received {}",
            validator.str_param("signal").unwrap_or_default()
        ),
        "service_active" => format!(
            "{} is active",
            validator.str_param("unit").unwrap_or_default()
        ),
        "service_inactive" => format!(
            "{} is stopped",
            validator.str_param("unit").unwrap_or_default()
        ),
        "service_enabled" => format!(
            "{} starts at boot",
            validator.str_param("unit").unwrap_or_default()
        ),
        "service_disabled" => format!(
            "{} does not start at boot",
            validator.str_param("unit").unwrap_or_default()
        ),
        "port_listening" => format!(
            "Something is listening on port {}",
            validator.i64_param("port").unwrap_or_default()
        ),
        "user_exists" => format!(
            "The account {} exists",
            validator.str_param("user").unwrap_or_default()
        ),
        "group_exists" => format!(
            "The group {} exists",
            validator.str_param("group").unwrap_or_default()
        ),
        "group_membership" => format!(
            "{} is a member of {}",
            validator.str_param("user").unwrap_or_default(),
            validator.str_param("group").unwrap_or_default()
        ),
        "package_installed" => format!(
            "The package {} is installed",
            validator.str_param("package").unwrap_or_default()
        ),
        "package_removed" => format!(
            "The package {} is removed",
            validator.str_param("package").unwrap_or_default()
        ),
        "script_exists" => format!("The script {path} exists"),
        "script_executable" => format!("{path} is executable"),
        "script_exit_code" => format!(
            "{path} exits with {}",
            validator.i64_param("exitCode").unwrap_or_default()
        ),
        "shellcheck_passes" => format!("{path} passes shellcheck"),
        "stdout_exact" | "stdout_contains" => {
            "The command produces the required output".to_string()
        }
        "stderr_contains" => "Errors go to the error stream".to_string(),
        "unit_test_passes" => {
            "Your solution works on hidden test data as well as the example".to_string()
        }
        "idempotent_result" => "Running it twice gives the same result".to_string(),
        // Anything not worth spelling out is hidden rather than shown as a raw tag.
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleView {
    pub id: String,
    pub number: u32,
    pub title: String,
    pub level: LessonLevel,
    pub summary: String,
    pub outcomes: Vec<String>,
    pub pack: String,
    pub lessons: Vec<LessonSummaryView>,
    pub completed_lessons: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LessonSummaryView {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub lesson_type: LessonType,
    pub estimated_difficulty: u8,
    pub estimated_minutes: Option<u32>,
    pub commands: Vec<String>,
    pub status: Option<String>,
    pub mastery: Option<MasteryStatus>,
    pub unlocked: bool,
    pub missing_prerequisites: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub app_version: String,
    pub runtime_version: String,
    pub profile_id: i64,
    pub mode: ProgressionMode,
    pub modules: Vec<ModuleView>,
    pub core_lesson_count: usize,
    pub completed_core_lessons: usize,
    pub mastery_percent: u8,
    pub next_lesson_id: Option<String>,
    pub review_lesson_ids: Vec<String>,
    pub recent_commands: Vec<String>,
    pub vm: VmStatus,
    pub acceleration: String,
    pub health: runtime_manager::HealthReport,
    pub catalog_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub validation: TaskValidation,
    pub headline: String,
    pub category: Option<FailureCategory>,
    pub guidance: Option<String>,
    pub details: Vec<String>,
    pub partial: bool,
    pub authoring_error: bool,
    /// Populated when this was the last outstanding task.
    pub lesson_complete: bool,
    pub mastery: Option<MasteryStatus>,
    /// Revealed only after a mistake task passes.
    pub diagnosis: Option<String>,
}

impl CheckResult {
    pub fn new(validation: TaskValidation, feedback: Feedback) -> Self {
        Self {
            validation,
            headline: feedback.headline,
            category: feedback.category,
            guidance: feedback.guidance,
            details: feedback.details,
            partial: feedback.partial,
            authoring_error: feedback.authoring_error,
            lesson_complete: false,
            mastery: None,
            diagnosis: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewGrade {
    pub correct: bool,
    pub explanation: Option<String>,
}

pub fn module_views(
    catalog: &Catalog,
    progress: &[LessonProgress],
    availability: impl Fn(&str) -> Availability,
) -> Vec<ModuleView> {
    catalog
        .modules()
        .map(|module: &Module| {
            let lessons: Vec<LessonSummaryView> = module
                .lessons
                .iter()
                .filter_map(|id| catalog.lesson(id))
                .map(|lesson| {
                    let record = progress.iter().find(|p| p.lesson_id == lesson.id);
                    let availability = availability(&lesson.id);
                    LessonSummaryView {
                        id: lesson.id.clone(),
                        title: lesson.title.clone(),
                        lesson_type: lesson.lesson_type,
                        estimated_difficulty: lesson.estimated_difficulty,
                        estimated_minutes: lesson.estimated_minutes,
                        commands: lesson.commands.clone(),
                        status: record.map(|r| {
                            serde_json::to_value(r.status)
                                .ok()
                                .and_then(|v| v.as_str().map(|s| s.to_string()))
                                .unwrap_or_default()
                        }),
                        mastery: record.and_then(|r| r.mastery),
                        unlocked: availability.unlocked,
                        missing_prerequisites: availability.missing_prerequisites,
                    }
                })
                .collect();
            let completed_lessons = lessons
                .iter()
                .filter(|l| matches!(l.status.as_deref(), Some("passed") | Some("needs-review")))
                .count();

            ModuleView {
                id: module.id.clone(),
                number: module.number,
                title: module.title.clone(),
                level: module.level,
                summary: module.summary.clone(),
                outcomes: module.outcomes.clone(),
                pack: module.pack.clone(),
                lessons,
                completed_lessons,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_types::{LessonContent, ResetPolicy, ReviewQuestion, Task, Validator};

    fn lesson(lesson_type: LessonType) -> Lesson {
        Lesson {
            schema_version: 1,
            id: "m.01".into(),
            title: "Creating directories".into(),
            level: LessonLevel::Beginner,
            module: "m".into(),
            lesson_type,
            estimated_difficulty: 1,
            estimated_minutes: Some(8),
            prerequisites: vec![],
            concepts: vec!["directories".into()],
            commands: vec!["mkdir".into()],
            environment: LessonEnvironment {
                profile: "filesystem-basic".into(),
                reset_policy: ResetPolicy::PerAttempt,
                network_mode: shared_types::NetworkMode::Disabled,
                sudo_allowed: false,
                dangerous_allowed: false,
                memory_mb: None,
                namespaces: vec![],
                setup_script: None,
                reset_script: None,
                fixtures: vec![],
            },
            content: LessonContent {
                purpose: "Organise files into directories.".into(),
                mental_model: "A directory is a container.".into(),
                syntax: vec!["mkdir DIRECTORY_NAME".into()],
                demonstration: vec![Demonstration {
                    command: "mkdir reports".into(),
                    explanation: Some("Creates a directory called reports.".into()),
                    output: None,
                }],
                explanation_markdown: None,
                summary: None,
            },
            tasks: vec![Task {
                id: "task-1".into(),
                kind: TaskKind::Guided,
                instruction: "Create a directory called reports.".into(),
                context: None,
                broken_command: None,
                diagnosis: None,
                validators: vec![
                    Validator::new("directory_exists").with("path", "/home/student/reports")
                ],
                hints: vec!["You need a command that creates directories.".into()],
                suggested_solution: Some("mkdir reports".into()),
                known_incorrect_solution: Some("touch reports".into()),
                alternate_solutions: vec!["mkdir -p ~/reports".into()],
                optional: false,
            }],
            review_questions: vec![ReviewQuestion {
                question_type: ReviewQuestionType::MultipleChoice,
                question: "What does ../ represent?".into(),
                answers: vec!["The parent directory".into(), "The home directory".into()],
                correct_answer: Some(0),
                correct_answers: vec![],
                accepted_answers: vec![],
                explanation: Some("It is the directory above the current one.".into()),
            }],
        }
    }

    #[test]
    fn the_lesson_payload_never_carries_solutions_or_answers() {
        let view = LessonView::from_lesson(
            &lesson(LessonType::GuidedPractice),
            ProgressionMode::GuidedPath,
        );
        let json = serde_json::to_string(&view).unwrap();

        // A learner with developer tools open must not be able to read the answer. The fields
        // themselves have to be absent: an empty or null field would still tell them a solution
        // exists and where to look for it.
        assert!(!json.contains("suggestedSolution"), "{json}");
        assert!(!json.contains("alternateSolutions"), "{json}");
        assert!(!json.contains("knownIncorrectSolution"), "{json}");
        assert!(!json.contains("correctAnswer"), "{json}");
        assert!(
            !json.contains("\"hints\":"),
            "hint text must be fetched one at a time"
        );
        assert!(
            !json.contains("You need a command that creates directories"),
            "{json}"
        );
        // The demonstration command is present on purpose: worked examples are teaching material,
        // and hiding them would defeat the lesson. It is the task's answer that stays behind an
        // explicit, recorded request.
        assert!(
            json.contains("mkdir reports"),
            "the demonstration should still be shown"
        );
    }

    #[test]
    fn hint_counts_are_exposed_so_the_ui_can_show_progress() {
        let view = LessonView::from_lesson(
            &lesson(LessonType::GuidedPractice),
            ProgressionMode::GuidedPath,
        );
        assert_eq!(view.tasks[0].hint_count, 1);
        assert!(view.hints_available);
    }

    #[test]
    fn assessments_hide_hints_syntax_and_worked_examples() {
        let view =
            LessonView::from_lesson(&lesson(LessonType::Assessment), ProgressionMode::GuidedPath);
        assert!(!view.hints_available);
        assert_eq!(view.tasks[0].hint_count, 0);
        assert!(view.syntax.is_empty());
        assert!(view.demonstration.is_empty());
    }

    #[test]
    fn assessment_mode_strips_examples_even_from_an_ordinary_lesson() {
        let view = LessonView::from_lesson(
            &lesson(LessonType::GuidedPractice),
            ProgressionMode::Assessment,
        );
        assert!(!view.hints_available);
        assert!(view.demonstration.is_empty());
    }

    #[test]
    fn requirements_state_the_goal_not_the_command() {
        let view = LessonView::from_lesson(
            &lesson(LessonType::GuidedPractice),
            ProgressionMode::GuidedPath,
        );
        let requirements = &view.tasks[0].requirements;
        assert_eq!(
            requirements,
            &vec!["The directory /home/student/reports exists".to_string()]
        );
        assert!(!requirements.iter().any(|r| r.contains("mkdir")));
    }

    #[test]
    fn review_questions_keep_their_options_but_lose_the_key() {
        let view = LessonView::from_lesson(
            &lesson(LessonType::GuidedPractice),
            ProgressionMode::GuidedPath,
        );
        assert_eq!(view.review_questions[0].answers.len(), 2);
        let json = serde_json::to_string(&view.review_questions).unwrap();
        assert!(!json.contains("explanation"), "{json}");
    }

    #[test]
    fn unrecognised_validators_are_omitted_rather_than_shown_as_raw_tags() {
        let validator = shared_types::Validator::new("apt_cache_updated");
        assert_eq!(describe_validator(&validator), None);
    }
}
