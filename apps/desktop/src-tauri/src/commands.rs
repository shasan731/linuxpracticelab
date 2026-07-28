//! The Tauri command surface.
//!
//! Every command returns `Result<T, String>` because Tauri needs a serialisable error, and the
//! string is what the learner reads. So the messages here are written for a person, not for a
//! log: they say what happened and what to do next.

use crate::agent::{expect_ok, AgentClient};
use crate::dto::{module_views, Bootstrap, CheckResult, LessonView, ReviewGrade};
use crate::state::{now_unix, AppState};
use lesson_engine::{
    build_validation_request, lesson_mastery, shape_feedback, HintOutcome, LessonState,
    ProgressIndex, Progression, ProgressionMode,
};
use runtime_manager::{
    install_bundled_runtime, reinstall_bundled_runtime, HealthCheck, InstallOutcome, Manifest,
};
use shared_types::protocol::{DirEntryInfo, GuestDiagnostics};
use shared_types::{
    AgentRequest, AgentResponse, LessonStatus, MasteryStatus, NetworkMode, ReviewQuestionType,
    TaskAttempt, VmStatus,
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, State};
use vm_manager::SessionKind;

type CommandResult<T> = Result<T, String>;

/// Renders an error chain for the UI. Uses `{:#}` so the cause is included, since "could not
/// create the overlay" alone is not actionable.
fn to_message(err: anyhow::Error) -> String {
    format!("{err:#}")
}

// ---------------------------------------------------------------------------
// Bootstrap and catalogue
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn install_runtime(state: State<'_, AppState>, force: bool) -> CommandResult<bool> {
    let layout = state.layout.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if force {
            reinstall_bundled_runtime(&layout)
        } else {
            install_bundled_runtime(&layout)
        }
    })
    .await
    .map_err(|error| format!("The runtime installer stopped unexpectedly: {error}"))?
    .map(|outcome| outcome == InstallOutcome::Installed)
    .map_err(to_message)
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>) -> CommandResult<Bootstrap> {
    let mode = *state.mode.read().await;
    let progress = {
        let store = state.store.lock().await;
        store
            .all_lesson_progress(state.profile_id)
            .map_err(to_message)?
    };

    let index: ProgressIndex = progress
        .iter()
        .map(|record| {
            (
                record.lesson_id.clone(),
                LessonState {
                    status: record.status,
                    mastery: record.mastery,
                },
            )
        })
        .collect();

    let progression = Progression::new(&state.catalog, mode);
    let modules = module_views(&state.catalog, &progress, |id| {
        progression.availability(id, &index)
    });

    let recent_commands = {
        let store = state.store.lock().await;
        store
            .command_history(state.profile_id, 20)
            .unwrap_or_default()
    };

    let (vm, acceleration) = {
        let manager = state.vm.lock().await;
        (manager.status(), manager.accel_decision().reason.clone())
    };

    let health = HealthCheck::new(&state.layout).run_with_unclean_shutdown(
        runtime_manager::free_disk_bytes(&state.layout.data_root),
        true,
        state.unclean_shutdown.load(Ordering::Relaxed),
    );

    Ok(Bootstrap {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime_version: state.layout.runtime_version.clone(),
        profile_id: state.profile_id,
        mode,
        modules,
        core_lesson_count: state.catalog.core_lesson_count(),
        completed_core_lessons: progression.completed_core_lessons(&index),
        mastery_percent: progression.mastery_percent(&index),
        next_lesson_id: progression.next_lesson(&index).map(|s| s.to_string()),
        review_lesson_ids: progression
            .review_recommendations(&index, 5)
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
        recent_commands,
        vm,
        acceleration,
        health,
        catalog_warnings: state.catalog.warnings().to_vec(),
    })
}

#[tauri::command]
pub async fn get_lesson(
    state: State<'_, AppState>,
    lesson_id: String,
) -> CommandResult<LessonView> {
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let mode = *state.mode.read().await;
    Ok(LessonView::from_lesson(lesson, mode))
}

#[tauri::command]
pub async fn set_progression_mode(
    state: State<'_, AppState>,
    mode: ProgressionMode,
) -> CommandResult<()> {
    *state.mode.write().await = mode;
    let store = state.store.lock().await;
    let rendered = serde_json::to_value(mode)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "guided-path".into());
    store
        .set_setting(state.profile_id, "progression.mode", &rendered)
        .map_err(to_message)
}

#[tauri::command]
pub async fn command_reference(state: State<'_, AppState>) -> CommandResult<Vec<CommandEntry>> {
    let mut entries: Vec<CommandEntry> = state
        .catalog
        .all_commands()
        .into_iter()
        .map(|command| CommandEntry {
            command: command.to_string(),
            lessons: state
                .catalog
                .lessons_for_command(command)
                .into_iter()
                .map(|lesson| (lesson.id.clone(), lesson.title.clone()))
                .collect(),
        })
        .collect();
    entries.sort_by(|a, b| a.command.cmp(&b.command));
    Ok(entries)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandEntry {
    pub command: String,
    pub lessons: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Virtual machine lifecycle
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_session(
    app: AppHandle,
    state: State<'_, AppState>,
    lesson_id: Option<String>,
) -> CommandResult<VmStatus> {
    start_session_inner(&app, &state, lesson_id).await
}

/// The body of `start_session`, callable from other commands.
///
/// Commands take `State`, which borrows from the request, so anything reused internally is
/// written against `&AppState` instead of trying to hand a `State` around.
async fn start_session_inner(
    app: &AppHandle,
    state: &AppState,
    lesson_id: Option<String>,
) -> CommandResult<VmStatus> {
    let layout = state.layout.clone();
    tauri::async_runtime::spawn_blocking(move || install_bundled_runtime(&layout))
        .await
        .map_err(|error| format!("The runtime installer stopped unexpectedly: {error}"))?
        .map_err(to_message)?;

    // A stale QEMU from a crash must go before anything touches the overlays.
    {
        let manager = state.vm.lock().await;
        if let Err(err) = manager.reap_stale_process() {
            tracing::warn!("could not check for a stale virtual machine: {err:#}");
        }
    }

    let (kind, network_mode, memory_mb) = match &lesson_id {
        Some(id) => {
            let lesson = state.catalog.try_lesson(id).map_err(to_message)?;
            (
                SessionKind::Lesson {
                    lesson_id: id.clone(),
                },
                lesson.environment.network_mode,
                lesson.environment.memory_mb,
            )
        }
        None => (SessionKind::FreePractice, NetworkMode::Disabled, None),
    };

    let mut config = {
        let mut manager = state.vm.lock().await;
        manager
            .prepare(&kind, network_mode, memory_mb)
            .await
            .map_err(to_message)?
    };

    {
        let mut manager = state.vm.lock().await;
        manager.start(&config).await.map_err(to_message)?;
    }

    let mut client = Arc::new(crate::agent::AgentClient::new(
        config.agent_port,
        config.control_token.clone(),
    ));
    *state.agent.write().await = Some(client.clone());
    *state.session.write().await = Some(kind.clone());

    // Attach the console before waiting for the agent, so the learner watches Linux boot
    // instead of staring at an empty panel.
    if let Err(err) = state.console.attach(app.clone(), config.console_port).await {
        tracing::warn!("could not attach the console: {err:#}");
    }

    // A Windows host can report WHPX as present even when it cannot run this QEMU/firmware
    // combination. Give the fast path a strict budget, then transparently retry with TCG.
    let first_budget = if config.accel == shared_types::AccelMode::Whpx {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(120)
    };
    let mut ready = wait_for_guest_or_qemu_exit(state, &client, first_budget).await;
    if ready.is_err() && config.accel == shared_types::AccelMode::Whpx {
        tracing::warn!("WHPX guest did not become ready; retrying with software translation");
        state.console.detach().await;
        *state.agent.write().await = None;

        {
            let mut manager = state.vm.lock().await;
            manager.stop().await.map_err(to_message)?;
            manager.use_software_acceleration("WHPX could not boot the guest");
            config = manager
                .prepare(&kind, network_mode, memory_mb)
                .await
                .map_err(to_message)?;
            manager.start(&config).await.map_err(to_message)?;
        }

        client = Arc::new(crate::agent::AgentClient::new(
            config.agent_port,
            config.control_token.clone(),
        ));
        *state.agent.write().await = Some(client.clone());
        if let Err(err) = state.console.attach(app.clone(), config.console_port).await {
            tracing::warn!("could not attach to the fallback Linux console: {err:#}");
        }
        ready = wait_for_guest_or_qemu_exit(state, &client, Duration::from_secs(120)).await;
    }

    match ready {
        Ok(AgentResponse::Pong {
            kernel,
            image_version,
            ..
        }) => {
            let mut manager = state.vm.lock().await;
            manager.mark_ready();
            let mut status = manager.status();
            status.guest_kernel = Some(kernel);
            status.image_version = Some(image_version);
            Ok(status)
        }
        Ok(other) => {
            let detail = format!("Linux returned an unexpected startup response: {other:?}");
            *state.agent.write().await = None;
            state.console.detach().await;
            let mut manager = state.vm.lock().await;
            manager.stop().await.map_err(to_message)?;
            manager.mark_failed(&detail);
            Err(detail)
        }
        Err(err) => {
            let mut detail = err.to_string();
            if let Some(console) = state.console.diagnostic_summary().await {
                detail.push_str(&format!(" Linux console: {console}"));
            }
            *state.agent.write().await = None;
            state.console.detach().await;
            let mut manager = state.vm.lock().await;
            manager.stop().await.map_err(to_message)?;
            manager.mark_failed(&detail);
            Err(detail)
        }
    }
}

async fn wait_for_guest_or_qemu_exit(
    state: &AppState,
    client: &AgentClient,
    budget: Duration,
) -> anyhow::Result<AgentResponse> {
    tokio::select! {
        ready = client.wait_until_ready(budget) => ready,
        exited = wait_for_qemu_exit(state) => {
            let status = exited?;
            let detail = last_vm_log_line(&state.layout.vm_log())
                .map(|line| format!(" The VM log says: {line}"))
                .unwrap_or_default();
            anyhow::bail!(
                "Linux stopped before it finished starting ({status}).{detail} \
                 Try Check practice environment; if the problem remains, reinstall the runtime."
            )
        }
    }
}

async fn wait_for_qemu_exit(state: &AppState) -> anyhow::Result<std::process::ExitStatus> {
    loop {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut manager = state.vm.lock().await;
        if let Some(status) = manager.poll_exited()? {
            return Ok(status);
        }
    }
}

fn last_vm_log_line(path: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    contents
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "**")
        .map(|line| line.chars().take(500).collect())
}

#[tauri::command]
pub async fn vm_status(state: State<'_, AppState>) -> CommandResult<VmStatus> {
    Ok(state.vm.lock().await.status())
}

#[tauri::command]
pub async fn stop_session(state: State<'_, AppState>) -> CommandResult<()> {
    stop_session_inner(&state).await
}

async fn stop_session_inner(state: &AppState) -> CommandResult<()> {
    // Ask the guest to shut down first so systemd flushes the journal; the host then powers
    // QEMU off regardless.
    if let Ok(client) = state.agent().await {
        let _ = client.request(AgentRequest::Shutdown).await;
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    state.console.detach().await;
    *state.agent.write().await = None;
    let mut manager = state.vm.lock().await;
    manager.stop().await.map_err(to_message)
}

#[tauri::command]
pub async fn restart_vm(app: AppHandle, state: State<'_, AppState>) -> CommandResult<VmStatus> {
    let session = state.session.read().await.clone();
    stop_session_inner(&state).await?;
    let lesson_id = match session {
        Some(SessionKind::Lesson { lesson_id }) => Some(lesson_id),
        _ => None,
    };
    start_session_inner(&app, &state, lesson_id).await
}

// ---------------------------------------------------------------------------
// Terminal
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn terminal_write(state: State<'_, AppState>, data: Vec<u8>) -> CommandResult<()> {
    state.console.write(&data).await.map_err(to_message)
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    rows: u16,
    cols: u16,
) -> CommandResult<()> {
    let client = state.agent().await.map_err(to_message)?;
    match client
        .request(AgentRequest::SetTerminalSize { rows, cols })
        .await
    {
        Ok(AgentResponse::TerminalResized { .. }) => Ok(()),
        Ok(AgentResponse::Error { message, .. }) => Err(message),
        // A resize that arrives while the guest is still booting is not worth surfacing.
        Ok(_) | Err(_) => Ok(()),
    }
}

/// Records a command the learner ran, subject to the retention policy.
#[tauri::command]
pub async fn record_command(state: State<'_, AppState>, command: String) -> CommandResult<bool> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    let store = state.store.lock().await;
    store
        .record_command(state.profile_id, &state.session_id, trimmed, now_unix())
        .map_err(to_message)
}

#[tauri::command]
pub async fn export_transcript(transcript: String) -> CommandResult<String> {
    // Redaction happens on the host so it cannot be skipped by the frontend.
    Ok(progress_store::redact_transcript(&transcript))
}

// ---------------------------------------------------------------------------
// Lessons
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn prepare_lesson(
    state: State<'_, AppState>,
    lesson_id: String,
) -> CommandResult<Vec<String>> {
    prepare_lesson_inner(&state, lesson_id).await
}

async fn prepare_lesson_inner(state: &AppState, lesson_id: String) -> CommandResult<Vec<String>> {
    state.require_ready().await.map_err(to_message)?;
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let client = state.agent().await.map_err(to_message)?;

    let response = client
        .request(AgentRequest::PrepareLesson {
            lesson_id: lesson_id.clone(),
            setup_script: lesson.environment.setup_script.clone(),
            fixtures: lesson.environment.fixtures.clone(),
            namespaces: lesson.environment.namespaces.clone(),
            sudo_allowed: lesson.environment.sudo_allowed,
        })
        .await
        .map_err(to_message)?;

    let warnings = match expect_ok(response).map_err(to_message)? {
        AgentResponse::LessonPrepared { warnings, .. } => warnings,
        other => {
            return Err(format!(
                "unexpected reply while preparing the lesson: {other:?}"
            ))
        }
    };

    {
        let store = state.store.lock().await;
        store
            .start_lesson(state.profile_id, &lesson_id, now_unix())
            .map_err(to_message)?;
    }
    Ok(warnings)
}

#[tauri::command]
pub async fn check_task(
    state: State<'_, AppState>,
    lesson_id: String,
    task_id: String,
) -> CommandResult<CheckResult> {
    state.require_ready().await.map_err(to_message)?;
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let task = lesson
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("lesson {lesson_id} has no task {task_id}"))?;

    // Re-check the validators every attempt: a lesson package is untrusted input, and a
    // malformed one must produce an authoring error rather than a pass.
    lesson_engine::check_task_validators(task).map_err(to_message)?;

    let started_at = now_unix();
    let request = build_validation_request(lesson, task, started_at);
    let client = state.agent().await.map_err(to_message)?;

    let response = client
        .request(AgentRequest::ValidateTask {
            lesson_id: lesson_id.clone(),
            task_id: task_id.clone(),
            validators: request.validators,
            subject_user: request.subject_user,
            attempt_started_at: request.attempt_started_at,
        })
        .await
        .map_err(to_message)?;

    let validation = match expect_ok(response).map_err(to_message)? {
        AgentResponse::TaskValidated(validation) => validation,
        other => {
            return Err(format!(
                "unexpected reply while checking the task: {other:?}"
            ))
        }
    };

    let feedback = shape_feedback(&validation);
    let mut result = CheckResult::new(validation.clone(), feedback);

    // Record the attempt.
    let (hints_used, category) = state
        .with_attempt(&lesson_id, &task_id, |attempt| {
            if validation.passed {
                attempt.record_pass();
            } else {
                attempt.record_failure(
                    validation
                        .primary_failure
                        .as_ref()
                        .and_then(|f| f.failure_category),
                );
            }
            (
                attempt.hints_revealed as u32,
                validation
                    .primary_failure
                    .as_ref()
                    .and_then(|f| f.failure_category),
            )
        })
        .await;

    {
        let store = state.store.lock().await;
        store
            .record_task_attempt(&TaskAttempt {
                id: None,
                profile_id: state.profile_id,
                lesson_id: lesson_id.clone(),
                task_id: task_id.clone(),
                passed: validation.passed,
                failure_category: category,
                hints_used,
                created_at: started_at,
            })
            .map_err(to_message)?;
    }

    if validation.passed {
        // A mistake task explains itself only once the learner has worked it out.
        result.diagnosis = task.diagnosis.clone();

        let attempts = state.lesson_attempts(&lesson_id).await;
        let required: Vec<&str> = lesson.required_tasks().map(|t| t.id.as_str()).collect();
        let passed_required = required
            .iter()
            .filter(|id| attempts.iter().any(|a| a.task_id == **id && a.passed))
            .count();

        if passed_required == required.len() && !required.is_empty() {
            let is_assessment = lesson.lesson_type.hides_hints();
            let mastery = lesson_mastery(&attempts, is_assessment);
            let total_hints = lesson_engine::hints::total_hints(&attempts);
            let solution_revealed = attempts.iter().any(|a| a.solution_revealed);

            let store = state.store.lock().await;
            let best = store
                .complete_lesson(
                    state.profile_id,
                    &lesson_id,
                    mastery,
                    total_hints,
                    solution_revealed,
                    now_unix(),
                )
                .map_err(to_message)?;
            drop(store);

            result.lesson_complete = true;
            result.mastery = Some(best);
        }
    } else {
        let store = state.store.lock().await;
        store
            .record_lesson_attempt(state.profile_id, &lesson_id, now_unix())
            .map_err(to_message)?;
    }

    Ok(result)
}

#[tauri::command]
pub async fn reveal_hint(
    state: State<'_, AppState>,
    lesson_id: String,
    task_id: String,
) -> CommandResult<HintResponse> {
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let task = lesson
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("lesson {lesson_id} has no task {task_id}"))?;

    let mode = *state.mode.read().await;
    let hints_allowed = !lesson.lesson_type.hides_hints() && mode != ProgressionMode::Assessment;

    let outcome = state
        .with_attempt(&lesson_id, &task_id, |attempt| {
            attempt.reveal_next_hint(task, hints_allowed)
        })
        .await;

    match outcome {
        HintOutcome::Hint(reveal) => {
            let store = state.store.lock().await;
            store
                .add_hint_used(state.profile_id, &lesson_id, now_unix())
                .map_err(to_message)?;
            Ok(HintResponse::Hint {
                index: reveal.index,
                text: reveal.text,
                remaining: reveal.remaining,
                solution_next: reveal.solution_next,
            })
        }
        HintOutcome::SolutionAvailable => Ok(HintResponse::SolutionAvailable),
        HintOutcome::Unavailable(reason) => Ok(HintResponse::Unavailable {
            reason: reason.to_string(),
        }),
    }
}

/// Struct variants rather than newtypes: serde cannot serialise an internally tagged newtype
/// variant that wraps a plain string, and that failure would only show up at runtime.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum HintResponse {
    #[serde(rename_all = "camelCase")]
    Hint {
        index: usize,
        text: String,
        remaining: usize,
        solution_next: bool,
    },
    SolutionAvailable,
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
    },
}

#[tauri::command]
pub async fn reveal_solution(
    state: State<'_, AppState>,
    lesson_id: String,
    task_id: String,
) -> CommandResult<SolutionResponse> {
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let task = lesson
        .tasks
        .iter()
        .find(|t| t.id == task_id)
        .ok_or_else(|| format!("lesson {lesson_id} has no task {task_id}"))?;

    let mode = *state.mode.read().await;
    let hints_allowed = !lesson.lesson_type.hides_hints() && mode != ProgressionMode::Assessment;

    let refusal = state
        .with_attempt(&lesson_id, &task_id, |attempt| {
            attempt.reveal_solution(hints_allowed)
        })
        .await;

    if let Some(reason) = refusal {
        return Ok(SolutionResponse {
            solution: None,
            reason: Some(reason.to_string()),
        });
    }

    Ok(SolutionResponse {
        // The learner still has to type it: this is text on screen, never sent to the shell.
        solution: task.suggested_solution.clone(),
        reason: None,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SolutionResponse {
    pub solution: Option<String>,
    pub reason: Option<String>,
}

#[tauri::command]
pub async fn grade_review_question(
    state: State<'_, AppState>,
    lesson_id: String,
    index: usize,
    selected: Option<Vec<usize>>,
    text: Option<String>,
) -> CommandResult<ReviewGrade> {
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let question = lesson
        .review_questions
        .get(index)
        .ok_or_else(|| format!("lesson {lesson_id} has no review question {index}"))?;

    let correct = match question.question_type {
        ReviewQuestionType::MultipleChoice => {
            selected.as_ref().and_then(|s| s.first().copied()) == question.correct_answer
        }
        ReviewQuestionType::MultipleSelect => {
            let mut given = selected.unwrap_or_default();
            given.sort_unstable();
            given.dedup();
            let mut expected = question.correct_answers.clone();
            expected.sort_unstable();
            given == expected
        }
        ReviewQuestionType::ShortAnswer | ReviewQuestionType::CommandRecall => {
            question.accepts_text(text.as_deref().unwrap_or_default())
        }
    };

    Ok(ReviewGrade {
        correct,
        // The explanation is worth showing either way; it is teaching, not the answer key.
        explanation: question.explanation.clone(),
    })
}

#[tauri::command]
pub async fn reset_lesson(state: State<'_, AppState>, lesson_id: String) -> CommandResult<()> {
    state.require_ready().await.map_err(to_message)?;
    let lesson = state.catalog.try_lesson(&lesson_id).map_err(to_message)?;
    let client = state.agent().await.map_err(to_message)?;

    let response = client
        .request(AgentRequest::ResetLesson {
            lesson_id: lesson_id.clone(),
            reset_script: lesson.environment.reset_script.clone(),
        })
        .await
        .map_err(to_message)?;
    expect_ok(response).map_err(to_message)?;
    Ok(())
}

/// Discards the lesson overlay and boots a fresh one. This is "Restart current lesson".
#[tauri::command]
pub async fn restart_lesson(
    app: AppHandle,
    state: State<'_, AppState>,
    lesson_id: String,
) -> CommandResult<VmStatus> {
    state.clear_lesson_attempts(&lesson_id).await;
    stop_session_inner(&state).await?;
    let status = start_session_inner(&app, &state, Some(lesson_id.clone())).await?;
    prepare_lesson_inner(&state, lesson_id).await?;
    Ok(status)
}

// ---------------------------------------------------------------------------
// Environment panels
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn guest_diagnostics(state: State<'_, AppState>) -> CommandResult<GuestDiagnostics> {
    state.require_ready().await.map_err(to_message)?;
    let client = state.agent().await.map_err(to_message)?;
    match client
        .request(AgentRequest::Diagnostics)
        .await
        .map_err(to_message)?
    {
        AgentResponse::Diagnostics(diagnostics) => Ok(diagnostics),
        AgentResponse::Error { message, .. } => Err(message),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

#[tauri::command]
pub async fn list_directory(
    state: State<'_, AppState>,
    path: String,
    include_hidden: bool,
) -> CommandResult<Vec<DirEntryInfo>> {
    state.require_ready().await.map_err(to_message)?;
    let client = state.agent().await.map_err(to_message)?;
    match client
        .request(AgentRequest::ListDirectory {
            path,
            include_hidden,
        })
        .await
        .map_err(to_message)?
    {
        AgentResponse::DirectoryListing { entries, .. } => Ok(entries),
        AgentResponse::Error { message, .. } => Err(message),
        other => Err(format!("unexpected reply: {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Snapshots and recovery
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_snapshot(state: State<'_, AppState>, name: String) -> CommandResult<String> {
    let live = state.layout.free_practice_overlay();
    if !live.exists() {
        return Err("There is no Free Practice environment to snapshot yet.".into());
    }

    let path = {
        let manager = state.vm.lock().await;
        manager
            .overlays()
            .snapshot(&live, &name)
            .map_err(to_message)?
    };

    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let store = state.store.lock().await;
    store
        .record_snapshot(
            state.profile_id,
            &name,
            &path.to_string_lossy(),
            &state.layout.runtime_version,
            size,
            now_unix(),
        )
        .map_err(to_message)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn list_snapshots(
    state: State<'_, AppState>,
) -> CommandResult<Vec<(i64, String, String, i64)>> {
    let store = state.store.lock().await;
    store.snapshots(state.profile_id).map_err(to_message)
}

#[tauri::command]
pub async fn restore_snapshot(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    // The VM must be down: restoring the overlay under a running guest corrupts it.
    stop_session_inner(&state).await?;

    let snapshots = {
        let store = state.store.lock().await;
        store.snapshots(state.profile_id).map_err(to_message)?
    };
    let (_, _, disk_path, _) = snapshots
        .into_iter()
        .find(|(snapshot_id, _, _, _)| *snapshot_id == id)
        .ok_or_else(|| "That snapshot no longer exists.".to_string())?;

    let manager = state.vm.lock().await;
    manager
        .overlays()
        .restore(
            std::path::Path::new(&disk_path),
            &state.layout.free_practice_overlay(),
        )
        .map_err(to_message)
}

/// Deletes the Free Practice overlay so the next start builds a clean one.
#[tauri::command]
pub async fn factory_reset_practice(state: State<'_, AppState>) -> CommandResult<()> {
    stop_session_inner(&state).await?;

    let live = state.layout.free_practice_overlay();
    if live.exists() {
        // Keep one automatic snapshot: spec 20 forbids leaving a learner with nothing to
        // restore, and a factory reset is exactly when that matters.
        let manager = state.vm.lock().await;
        if let Err(err) = manager.overlays().snapshot(&live, "before-factory-reset") {
            tracing::warn!("could not snapshot before the factory reset: {err:#}");
        }
        drop(manager);
        std::fs::remove_file(&live)
            .map_err(|err| format!("could not remove the practice environment: {err}"))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn verify_runtime(state: State<'_, AppState>) -> CommandResult<Vec<String>> {
    let manifest = Manifest::load(&state.layout.checksums_file()).map_err(to_message)?;
    let report = manifest.verify(&state.layout.runtime_root);
    Ok(report.problems())
}

#[tauri::command]
pub async fn health_check(
    state: State<'_, AppState>,
) -> CommandResult<runtime_manager::HealthReport> {
    if state.unclean_shutdown.load(Ordering::Relaxed) {
        let manager = state.vm.lock().await;
        if matches!(
            manager.state(),
            shared_types::VmState::Stopped
                | shared_types::VmState::Failed
                | shared_types::VmState::Unbootable
        ) {
            let spec = manager
                .overlays()
                .free_practice_spec(&state.layout.base_image());
            if spec.path.exists() {
                manager.overlays().ensure(&spec).await.map_err(to_message)?;
            }
            state.unclean_shutdown.store(false, Ordering::Relaxed);
        }
    }

    Ok(HealthCheck::new(&state.layout).run_with_unclean_shutdown(
        runtime_manager::free_disk_bytes(&state.layout.data_root),
        false,
        state.unclean_shutdown.load(Ordering::Relaxed),
    ))
}

// ---------------------------------------------------------------------------
// Progress and settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn progress_report(state: State<'_, AppState>) -> CommandResult<ProgressReport> {
    let store = state.store.lock().await;
    let lessons = store
        .all_lesson_progress(state.profile_id)
        .map_err(to_message)?;
    let failures = store
        .common_failures(state.profile_id, 8)
        .map_err(to_message)?;
    let achievements = store.achievements(state.profile_id).map_err(to_message)?;
    let practice_seconds = store
        .metric(state.profile_id, "practice.seconds")
        .unwrap_or(0);
    drop(store);

    let mastered = lessons
        .iter()
        .filter(|l| l.mastery == Some(MasteryStatus::Mastered))
        .count();
    let needs_review = lessons
        .iter()
        .filter(|l| l.status == LessonStatus::NeedsReview)
        .count();

    let mut commands_mastered: Vec<String> = lessons
        .iter()
        .filter(|l| l.mastery.map(|m| !m.needs_revisiting()).unwrap_or(false))
        .filter_map(|l| state.catalog.lesson(&l.lesson_id))
        .flat_map(|lesson| lesson.commands.clone())
        .collect();
    commands_mastered.sort();
    commands_mastered.dedup();

    Ok(ProgressReport {
        lessons_attempted: lessons.len(),
        lessons_passed: lessons
            .iter()
            .filter(|l| matches!(l.status, LessonStatus::Passed | LessonStatus::NeedsReview))
            .count(),
        lessons_mastered: mastered,
        needs_review,
        hints_used: lessons.iter().map(|l| l.hints_used).sum(),
        commands_mastered,
        common_failures: failures,
        achievements,
        practice_seconds,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressReport {
    pub lessons_attempted: usize,
    pub lessons_passed: usize,
    pub lessons_mastered: usize,
    pub needs_review: usize,
    pub hints_used: u32,
    pub commands_mastered: Vec<String>,
    pub common_failures: Vec<(String, u32)>,
    pub achievements: Vec<(String, i64)>,
    pub practice_seconds: i64,
}

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> CommandResult<Option<String>> {
    let store = state.store.lock().await;
    store.setting(state.profile_id, &key).map_err(to_message)
}

#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> CommandResult<()> {
    {
        let store = state.store.lock().await;
        store
            .set_setting(state.profile_id, &key, &value)
            .map_err(to_message)?;
        // Reducing history retention must take effect immediately, not at the next launch.
        if key == "history.retention" {
            store
                .prune_command_history(state.profile_id, &state.session_id, now_unix())
                .map_err(to_message)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn bump_practice_time(state: State<'_, AppState>, seconds: i64) -> CommandResult<i64> {
    let store = state.store.lock().await;
    store
        .bump_metric(state.profile_id, "practice.seconds", seconds.clamp(0, 3600))
        .map_err(to_message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_include_the_cause() {
        let err = anyhow::anyhow!("could not create the overlay")
            .context("starting Free Practice failed");
        let message = to_message(err);
        assert!(message.contains("starting Free Practice failed"));
        assert!(message.contains("could not create the overlay"));
    }

    #[test]
    fn vm_log_summary_skips_empty_assertion_markers() {
        let log = std::env::temp_dir().join(format!(
            "linux-practice-lab-vm-log-test-{}.log",
            std::process::id()
        ));
        std::fs::write(&log, "useful failure detail\n\n**\n").unwrap();
        assert_eq!(
            last_vm_log_line(&log).as_deref(),
            Some("useful failure detail")
        );
        std::fs::remove_file(log).ok();
    }
}
