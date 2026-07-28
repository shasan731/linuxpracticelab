//! Linux Practice Lab desktop application.
//!
//! Startup order matters and is deliberate: work out where everything lives, verify the
//! runtime, open the local database, load the lesson catalogue, and only then show a window.
//! The virtual machine is not started here — it starts when the learner opens a lesson or Free
//! Practice, which is what keeps a cold launch fast and avoids burning a guest boot on someone
//! who only wanted to read the command reference.

mod agent;
mod commands;
mod console;
mod dto;
mod state;

use anyhow::{Context, Result};
use lesson_engine::Catalog;
use progress_store::ProgressStore;
use runtime_manager::{HealthCheck, Layout, RUNTIME_VERSION};
use state::{now_unix, AppState};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(err) = try_run() {
        // Nothing else has a window yet, so the failure has to be reported here.
        eprintln!("Linux Practice Lab could not start: {err:#}");
        std::process::exit(1);
    }
}

fn try_run() -> Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let resource_dir = app
                .path()
                .resource_dir()
                .context("could not find the application resource directory")?;

            let layout = Layout::discover(RUNTIME_VERSION, &resource_dir)?;
            layout.ensure_writable_dirs()?;

            init_logging(&layout);

            let health = HealthCheck::new(&layout);
            let unclean_shutdown = health.has_session_lock();
            // Written before anything else so a crash from here on is detectable next launch.
            health.acquire_session_lock().ok();

            let catalog = Catalog::load(&layout.lessons_root).with_context(|| {
                format!(
                    "could not load the lesson catalogue from {}",
                    layout.lessons_root.display()
                )
            })?;
            for warning in catalog.warnings() {
                tracing::warn!("lesson catalogue: {warning}");
            }
            tracing::info!(
                "loaded {} lessons ({} in the core path) across {} modules",
                catalog.lesson_count(),
                catalog.core_lesson_count(),
                catalog.modules().count()
            );

            let store = ProgressStore::open(&layout.progress_db())?;
            let profile_id = store.ensure_profile("student", now_unix())?;
            // Enforce the retention policy at startup, so a learner who reduced it last session
            // sees the old history actually gone.
            let session_scope = format!("{}-startup", std::process::id());
            store
                .prune_command_history(profile_id, &session_scope, now_unix())
                .ok();

            let mode = store
                .setting(profile_id, "progression.mode")
                .ok()
                .flatten()
                .and_then(|value| serde_json::from_value(serde_json::Value::String(value)).ok())
                .unwrap_or_default();

            let app_state = AppState::new(layout, catalog, store, profile_id, unclean_shutdown);
            {
                // `blocking_write` is safe in setup: no async task exists yet to contend for it.
                *app_state.mode.blocking_write() = mode;
            }
            app.manage(app_state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                // Best-effort tidy-up. The Job Object is what actually guarantees QEMU dies
                // with us; this only releases the lock so the next launch does not warn.
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    HealthCheck::new(&state.layout).release_session_lock();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::install_runtime,
            commands::get_lesson,
            commands::set_progression_mode,
            commands::command_reference,
            commands::start_session,
            commands::vm_status,
            commands::stop_session,
            commands::restart_vm,
            commands::terminal_write,
            commands::terminal_resize,
            commands::record_command,
            commands::export_transcript,
            commands::prepare_lesson,
            commands::check_task,
            commands::reveal_hint,
            commands::reveal_solution,
            commands::grade_review_question,
            commands::reset_lesson,
            commands::restart_lesson,
            commands::guest_diagnostics,
            commands::list_directory,
            commands::create_snapshot,
            commands::list_snapshots,
            commands::restore_snapshot,
            commands::factory_reset_practice,
            commands::verify_runtime,
            commands::health_check,
            commands::progress_report,
            commands::get_setting,
            commands::set_setting,
            commands::bump_practice_time,
        ])
        .run(tauri::generate_context!())
        .context("the application exited unexpectedly")?;
    Ok(())
}

/// Logs to a rolling file plus stderr. Kept out of the user data directory's way so a
/// disk-full lesson cannot be confused with the application filling the disk itself.
fn init_logging(layout: &Layout) {
    use tracing_subscriber::prelude::*;

    let file_appender = tracing_appender::rolling::daily(layout.logs_dir(), "application.log");
    let filter = tracing_subscriber::EnvFilter::try_from_env("LINUXLAB_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_appender)
                .with_ansi(false),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .try_init();
}
