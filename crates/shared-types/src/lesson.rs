//! Lesson package types. These mirror `lessons/schema/lesson.schema.json` exactly;
//! CI validates the JSON against the schema and these structs deserialise the same files,
//! so a drift between the two shows up as a failing test rather than a runtime surprise.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub level: LessonLevel,
    pub module: String,
    #[serde(rename = "type")]
    pub lesson_type: LessonType,
    pub estimated_difficulty: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_minutes: Option<u32>,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    pub concepts: Vec<String>,
    #[serde(default)]
    pub commands: Vec<String>,
    pub environment: LessonEnvironment,
    pub content: LessonContent,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub review_questions: Vec<ReviewQuestion>,
}

impl Lesson {
    /// Tasks that must pass for the lesson to count as complete.
    pub fn required_tasks(&self) -> impl Iterator<Item = &Task> {
        self.tasks.iter().filter(|t| !t.optional)
    }

    /// Total hints available across required tasks. Used to scale mastery scoring.
    pub fn total_hints(&self) -> usize {
        self.required_tasks().map(|t| t.hints.len()).sum()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum LessonLevel {
    Orientation,
    Beginner,
    Foundation,
    Intermediate,
    Advanced,
    Administrator,
    Troubleshooting,
    Capstone,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum LessonType {
    Concept,
    Demonstration,
    GuidedPractice,
    IndependentPractice,
    Debugging,
    Scenario,
    Review,
    Assessment,
    Capstone,
}

impl LessonType {
    /// Assessment-style lessons hide hints and worked examples (spec 9.7).
    pub fn hides_hints(self) -> bool {
        matches!(self, LessonType::Assessment | LessonType::Capstone)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LessonEnvironment {
    pub profile: String,
    pub reset_policy: ResetPolicy,
    pub network_mode: NetworkMode,
    pub sudo_allowed: bool,
    #[serde(default)]
    pub dangerous_allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub namespaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_script: Option<String>,
    #[serde(default)]
    pub fixtures: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResetPolicy {
    PerAttempt,
    PerLesson,
    Manual,
    Never,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    Disabled,
    InternalLab,
    RestrictedInternet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LessonContent {
    pub purpose: String,
    pub mental_model: String,
    #[serde(default)]
    pub syntax: Vec<String>,
    #[serde(default)]
    pub demonstration: Vec<Demonstration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<LessonSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Demonstration {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LessonSummary {
    #[serde(default)]
    pub remember: Vec<String>,
    #[serde(default)]
    pub common_options: Vec<CommandOption>,
    #[serde(default)]
    pub dangerous: Vec<String>,
    #[serde(default)]
    pub related: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandOption {
    pub option: String,
    pub meaning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub kind: TaskKind,
    pub instruction: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broken_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnosis: Option<String>,
    pub validators: Vec<Validator>,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_solution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_incorrect_solution: Option<String>,
    #[serde(default)]
    pub alternate_solutions: Vec<String>,
    #[serde(default)]
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    Guided,
    Independent,
    Mistake,
    Applied,
    Assessment,
}

/// A validator is a tagged bag of parameters. The set of legal tags and their parameters
/// lives in `lessons/schema/validators.json` so that the authoring tooling and the guest
/// agent cannot disagree; keeping the params untyped here is deliberate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Validator {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub params: BTreeMap<String, serde_json::Value>,
}

impl Validator {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    pub fn str_param(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(|v| v.as_str())
    }

    pub fn i64_param(&self, key: &str) -> Option<i64> {
        self.params.get(key).and_then(|v| v.as_i64())
    }

    pub fn bool_param(&self, key: &str) -> Option<bool> {
        self.params.get(key).and_then(|v| v.as_bool())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewQuestion {
    #[serde(rename = "type")]
    pub question_type: ReviewQuestionType,
    pub question: String,
    #[serde(default)]
    pub answers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct_answer: Option<usize>,
    #[serde(default)]
    pub correct_answers: Vec<usize>,
    #[serde(default)]
    pub accepted_answers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewQuestionType {
    MultipleChoice,
    MultipleSelect,
    ShortAnswer,
    CommandRecall,
}

impl ReviewQuestion {
    /// Grades a free-text answer by collapsing whitespace and ignoring case, so
    /// `mkdir  reports` and `MKDIR reports` both count.
    pub fn accepts_text(&self, given: &str) -> bool {
        let normalise = |s: &str| {
            s.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
        };
        let given = normalise(given);
        self.accepted_answers
            .iter()
            .any(|candidate| normalise(candidate) == given)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub schema_version: u32,
    pub id: String,
    pub number: u32,
    pub title: String,
    pub level: LessonLevel,
    pub summary: String,
    #[serde(default)]
    pub outcomes: Vec<String>,
    #[serde(default = "default_pack")]
    pub pack: String,
    pub lessons: Vec<String>,
}

fn default_pack() -> String {
    "core".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validator_roundtrips_with_flattened_params() {
        let v = Validator::new("file_exists").with("path", "/home/student/a.txt");
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(
            json,
            r#"{"type":"file_exists","path":"/home/student/a.txt"}"#
        );

        let back: Validator = serde_json::from_str(&json).unwrap();
        assert_eq!(back.kind, "file_exists");
        assert_eq!(back.str_param("path"), Some("/home/student/a.txt"));
    }

    #[test]
    fn short_answer_grading_ignores_case_and_extra_spaces() {
        let q = ReviewQuestion {
            question_type: ReviewQuestionType::CommandRecall,
            question: "Which command creates a directory?".into(),
            answers: vec![],
            correct_answer: None,
            correct_answers: vec![],
            accepted_answers: vec!["mkdir".into(), "mkdir -p".into()],
            explanation: None,
        };
        assert!(q.accepts_text("MkDir"));
        assert!(q.accepts_text("  mkdir   -p "));
        assert!(!q.accepts_text("rmdir"));
    }

    #[test]
    fn assessment_lessons_hide_hints() {
        assert!(LessonType::Assessment.hides_hints());
        assert!(LessonType::Capstone.hides_hints());
        assert!(!LessonType::GuidedPractice.hides_hints());
    }
}
