//! Application state shared by every Tauri command.

use crate::agent::AgentClient;
use crate::console::ConsoleBridge;
use anyhow::{Context, Result};
use lesson_engine::{AttemptState, Catalog, ProgressionMode};
use progress_store::ProgressStore;
use runtime_manager::Layout;
use shared_types::VmState;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use vm_manager::{SessionKind, VmManager};

pub struct AppState {
    pub layout: Layout,
    pub catalog: Arc<Catalog>,
    pub store: Mutex<ProgressStore>,
    pub vm: Mutex<VmManager>,
    pub console: ConsoleBridge,
    pub agent: RwLock<Option<Arc<AgentClient>>>,
    pub session: RwLock<Option<SessionKind>>,
    pub profile_id: i64,
    /// Per-task attempt state, keyed by `lesson_id/task_id`. Lives in memory because it is
    /// scoped to one sitting; the durable record is written to the database on completion.
    pub attempts: Mutex<HashMap<String, AttemptState>>,
    pub mode: RwLock<ProgressionMode>,
    /// Identifier for this run, used to scope command history retention.
    pub session_id: String,
    /// Captured before this process writes its own lock. Cleared after the overlay check.
    pub unclean_shutdown: AtomicBool,
}

impl AppState {
    pub fn new(
        layout: Layout,
        catalog: Catalog,
        store: ProgressStore,
        profile_id: i64,
        unclean_shutdown: bool,
    ) -> Self {
        let paths = vm_manager::RuntimePaths {
            qemu_system: layout.qemu_system(),
            qemu_img: layout.qemu_img(),
            kernel: layout.kernel(),
            initrd: layout.initrd().exists().then(|| layout.initrd()),
            base_image: layout.base_image(),
            data_dir: layout.user_data_dir(),
            log_dir: layout.logs_dir(),
        };
        Self {
            layout,
            catalog: Arc::new(catalog),
            store: Mutex::new(store),
            vm: Mutex::new(VmManager::new(paths)),
            console: ConsoleBridge::new(),
            agent: RwLock::new(None),
            session: RwLock::new(None),
            profile_id,
            attempts: Mutex::new(HashMap::new()),
            mode: RwLock::new(ProgressionMode::GuidedPath),
            session_id: unique_session_id(),
            unclean_shutdown: AtomicBool::new(unclean_shutdown),
        }
    }

    pub async fn agent(&self) -> Result<Arc<AgentClient>> {
        self.agent
            .read()
            .await
            .clone()
            .context("Linux is not running yet. Open a lesson or Free Practice to start it.")
    }

    /// Refuses guest work unless the VM is genuinely ready, so a command issued during boot
    /// produces a clear message instead of a connection error.
    pub async fn require_ready(&self) -> Result<()> {
        let state = self.vm.lock().await.state();
        if state.accepts_commands() {
            return Ok(());
        }
        let message = match state {
            VmState::Stopped => "Linux is not running yet.",
            VmState::Starting | VmState::BootingGuest => "Linux is still starting.",
            VmState::Paused => "Linux is paused. Resume it to carry on.",
            VmState::Stopping => "Linux is shutting down.",
            VmState::Unbootable => {
                "The Linux practice environment is no longer bootable. Restore a snapshot or \
                 create a fresh environment."
            }
            VmState::Failed => "Linux could not be started. See the Environment panel for details.",
            VmState::Ready => unreachable!(),
        };
        anyhow::bail!(message)
    }

    pub fn attempt_key(lesson_id: &str, task_id: &str) -> String {
        format!("{lesson_id}/{task_id}")
    }

    /// Fetches or creates the in-memory attempt record for a task.
    pub async fn with_attempt<T>(
        &self,
        lesson_id: &str,
        task_id: &str,
        f: impl FnOnce(&mut AttemptState) -> T,
    ) -> T {
        let mut attempts = self.attempts.lock().await;
        let entry = attempts
            .entry(Self::attempt_key(lesson_id, task_id))
            .or_insert_with(|| AttemptState::new(lesson_id, task_id));
        f(entry)
    }

    /// Every attempt for a lesson, used when rolling a lesson up into a mastery result.
    pub async fn lesson_attempts(&self, lesson_id: &str) -> Vec<AttemptState> {
        let prefix = format!("{lesson_id}/");
        self.attempts
            .lock()
            .await
            .iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .map(|(_, value)| value.clone())
            .collect()
    }

    /// Clears attempt state for a lesson. Called on restart so a fresh run is scored fresh.
    pub async fn clear_lesson_attempts(&self, lesson_id: &str) {
        let prefix = format!("{lesson_id}/");
        self.attempts
            .lock()
            .await
            .retain(|key, _| !key.starts_with(&prefix));
    }
}

/// Session identifier derived from the process id and the current time.
fn unique_session_id() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}-{}", std::process::id(), seconds)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempt_keys_are_scoped_per_lesson_and_task() {
        assert_eq!(
            AppState::attempt_key("filesystem.navigation.04", "task-1"),
            "filesystem.navigation.04/task-1"
        );
        assert_ne!(
            AppState::attempt_key("a", "task-1"),
            AppState::attempt_key("b", "task-1")
        );
    }

    #[test]
    fn session_ids_differ_between_runs() {
        // Includes the pid, so two concurrent installations cannot share history scope.
        assert!(unique_session_id().contains(&std::process::id().to_string()));
    }
}
