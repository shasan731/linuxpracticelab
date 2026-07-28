//! Types shared across the host application, the guest agent and the frontend.
//!
//! Everything here is `serde`-serialisable because the same shapes travel over three
//! boundaries: Tauri IPC to the Svelte frontend, the authenticated control channel to
//! the guest agent, and the on-disk lesson packages.

pub mod lesson;
pub mod progress;
pub mod protocol;
pub mod registry;
pub mod validation;
pub mod vm;

pub use lesson::{
    Demonstration, Lesson, LessonContent, LessonEnvironment, LessonLevel, LessonSummary,
    LessonType, Module, NetworkMode, ResetPolicy, ReviewQuestion, ReviewQuestionType, Task,
    TaskKind, Validator,
};
pub use progress::{LessonProgress, LessonStatus, MasteryStatus, ModuleProgress, TaskAttempt};
pub use protocol::{AgentRequest, AgentResponse, RequestEnvelope, ResponseEnvelope};
pub use registry::{registry, Registry, RegistryError, ValidatorSpec};
pub use validation::{
    CheckOutcome, FailureCategory, TaskValidation, ValidationRequest, ValidationSummary,
};
pub use vm::{AccelMode, MachineType, VmConfig, VmState, VmStatus};
