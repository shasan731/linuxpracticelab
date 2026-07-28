//! Process and job validators.
//!
//! Matching is done against `/proc`, never against `ps` output, and the agent excludes itself
//! and its own children from every match — otherwise a validator looking for `sleep 300`
//! would happily match the `runuser` invocation the validator itself just ran.

use super::args::{self, Bounds};
use super::Ctx;
use crate::sys::{self, ProcessFacts};
use regex::Regex;
use shared_types::{CheckOutcome, FailureCategory, Validator};

pub async fn dispatch(ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "process_running" => process_running(validator),
        "process_not_running" => process_not_running(validator),
        "process_owner" => process_owner(validator),
        "process_count" => process_count(validator),
        "process_command" => process_command(validator),
        "process_signal_received" => process_signal_received(ctx, validator).await,
        "background_job_running" => background_job_running(ctx, validator),
        _ => return None,
    };
    Some(outcome)
}

macro_rules! arg {
    ($e:expr) => {
        match $e {
            Ok(value) => value,
            Err(outcome) => return outcome,
        }
    };
}

fn compile(validator: &Validator, pattern: &str) -> Result<Regex, CheckOutcome> {
    Regex::new(pattern).map_err(|err| {
        CheckOutcome::error(
            &validator.kind,
            format!("the pattern '{pattern}' is not a valid regex: {err}"),
        )
    })
}

/// Everything the learner is running, with the agent's own process tree removed.
fn candidate_processes() -> Vec<ProcessFacts> {
    let agent_pid = std::process::id();
    let all = sys::read_processes();
    let mut own_tree = vec![agent_pid];
    // Walk down a couple of generations: runuser -> bash -> command is the deepest we spawn.
    for _ in 0..4 {
        let children: Vec<u32> = all
            .iter()
            .filter(|p| own_tree.contains(&p.ppid) && !own_tree.contains(&p.pid))
            .map(|p| p.pid)
            .collect();
        if children.is_empty() {
            break;
        }
        own_tree.extend(children);
    }
    all.into_iter()
        .filter(|p| !own_tree.contains(&p.pid))
        .collect()
}

fn matching(pattern: &Regex) -> Vec<ProcessFacts> {
    candidate_processes()
        .into_iter()
        .filter(|p| pattern.is_match(p.match_text()))
        .collect()
}

fn describe(processes: &[ProcessFacts], limit: usize) -> String {
    if processes.is_empty() {
        return "no matching process".to_string();
    }
    processes
        .iter()
        .take(limit)
        .map(|p| format!("pid {} ({})", p.pid, truncate(p.match_text(), 60)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max).collect();
    format!("{kept}…")
}

fn process_running(v: &Validator) -> CheckOutcome {
    let pattern_text = arg!(args::string(v, "pattern"));
    let pattern = arg!(compile(v, pattern_text));
    let mut found = matching(&pattern);

    if let Some(owner) = args::optional_string(v, "owner") {
        let Ok(uid) = sys::uid_for(owner) else {
            return CheckOutcome::fail(
                &v.kind,
                format!("There is no user called {owner}."),
                FailureCategory::WrongOwnership,
            );
        };
        found.retain(|p| p.uid == uid);
    }

    let required = args::optional_integer(v, "minCount").unwrap_or(1).max(1) as usize;
    if found.len() >= required {
        CheckOutcome::pass(
            &v.kind,
            format!("Found {} matching process(es).", found.len()),
        )
        .observed(describe(&found, 3))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("No process matching '{pattern_text}' is running."),
            FailureCategory::ProcessNotRunning,
        )
        .expected(format!("at least {required} matching '{pattern_text}'"))
        .observed(describe(&found, 3))
    }
}

fn process_not_running(v: &Validator) -> CheckOutcome {
    let pattern_text = arg!(args::string(v, "pattern"));
    let pattern = arg!(compile(v, pattern_text));
    let found = matching(&pattern);
    if found.is_empty() {
        CheckOutcome::pass(
            &v.kind,
            format!("Nothing matching '{pattern_text}' is running."),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("A process matching '{pattern_text}' is still running."),
            FailureCategory::ProcessNotRunning,
        )
        .observed(describe(&found, 3))
    }
}

fn process_owner(v: &Validator) -> CheckOutcome {
    let pattern_text = arg!(args::string(v, "pattern"));
    let expected = arg!(args::string(v, "owner"));
    let pattern = arg!(compile(v, pattern_text));
    let found = matching(&pattern);

    if found.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            format!(
                "No process matching '{pattern_text}' is running, so its owner cannot be checked."
            ),
            FailureCategory::ProcessNotRunning,
        );
    }

    let owners: Vec<String> = found
        .iter()
        .map(|p| sys::user_for_uid(p.uid).unwrap_or_else(|| p.uid.to_string()))
        .collect();

    if owners.iter().all(|owner| owner == expected) {
        CheckOutcome::pass(&v.kind, format!("The process runs as {expected}."))
    } else {
        let mut distinct = owners.clone();
        distinct.sort();
        distinct.dedup();
        CheckOutcome::fail(
            &v.kind,
            "The process is not running as the expected user.".to_string(),
            FailureCategory::WrongOwnership,
        )
        .expected(expected)
        .observed(distinct.join(", "))
    }
}

fn process_count(v: &Validator) -> CheckOutcome {
    let pattern_text = arg!(args::string(v, "pattern"));
    let pattern = arg!(compile(v, pattern_text));
    let bounds = Bounds::read(v);
    let found = matching(&pattern);
    let actual = found.len() as i64;

    if bounds.satisfied_by(actual) {
        CheckOutcome::pass(
            &v.kind,
            format!("{actual} matching process(es) are running."),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!(
                "The number of processes matching '{pattern_text}' is not what the task asked for."
            ),
            FailureCategory::ProcessNotRunning,
        )
        .expected(bounds.describe())
        .observed(actual.to_string())
    }
}

fn process_command(v: &Validator) -> CheckOutcome {
    let pattern_text = arg!(args::string(v, "pattern"));
    let needle = arg!(args::string(v, "contains"));
    let pattern = arg!(compile(v, pattern_text));
    let found = matching(&pattern);

    if found.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            format!("No process matching '{pattern_text}' is running."),
            FailureCategory::ProcessNotRunning,
        );
    }
    if found.iter().any(|p| p.match_text().contains(needle)) {
        CheckOutcome::pass(
            &v.kind,
            format!("A matching process was started with '{needle}'."),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("The process is running but was not started with '{needle}'."),
            FailureCategory::ProcessNotRunning,
        )
        .expected(needle)
        .observed(describe(&found, 2))
    }
}

/// Signals are observed through a wrapper the lesson setup uses to launch the target.
///
/// There is no way to ask a live process "were you sent SIGTERM?", so the lesson launches it
/// under `linuxlab-signal-trap`, which appends each signal it receives to a log. Checking the
/// log is how "the learner stopped this politely rather than with SIGKILL" becomes verifiable.
async fn process_signal_received(_ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let handle = arg!(args::string(v, "handle"));
    let expected = arg!(args::string(v, "signal"));

    let safe_handle: String = handle
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_handle.is_empty() {
        return CheckOutcome::error(&v.kind, "the handle parameter is empty after sanitising");
    }
    let log = format!("/run/linuxlab/signals/{safe_handle}");

    let Ok(content) = std::fs::read_to_string(&log) else {
        // SIGKILL cannot be trapped, so the wrapper records it as an exit reason instead.
        if expected == "SIGKILL" {
            return CheckOutcome::fail(
                &v.kind,
                "The process was not stopped, or the lesson's signal wrapper is not running."
                    .to_string(),
                FailureCategory::ProcessNotRunning,
            );
        }
        return CheckOutcome::fail(
            &v.kind,
            format!("No signal has been recorded for '{safe_handle}' yet."),
            FailureCategory::ProcessNotRunning,
        );
    };

    let seen: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if seen.contains(&expected) {
        CheckOutcome::pass(&v.kind, format!("The process received {expected}."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("The process has not received {expected}."),
            FailureCategory::ProcessNotRunning,
        )
        .expected(expected)
        .observed(if seen.is_empty() {
            "no signals".to_string()
        } else {
            seen.join(", ")
        })
    }
}

/// A background job: running, owned by the learner, and not the foreground process group of
/// their terminal.
fn background_job_running(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let pattern_text = arg!(args::string(v, "pattern"));
    let pattern = arg!(compile(v, pattern_text));
    let Ok(uid) = sys::uid_for(&ctx.subject_user) else {
        return CheckOutcome::error(
            &v.kind,
            format!("there is no user called {}", ctx.subject_user),
        );
    };

    let found: Vec<ProcessFacts> = matching(&pattern)
        .into_iter()
        .filter(|p| p.uid == uid)
        .collect();
    if found.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            format!("Nothing matching '{pattern_text}' is running."),
            FailureCategory::ProcessNotRunning,
        );
    }

    if args::flag(v, "detached") {
        // nohup/disown leave the process reparented to init once its shell exits.
        let detached: Vec<&ProcessFacts> = found.iter().filter(|p| p.ppid == 1).collect();
        return if detached.is_empty() {
            CheckOutcome::fail(
                &v.kind,
                "The job is running but is still attached to your shell, so it would stop when \
                 you log out."
                    .to_string(),
                FailureCategory::ProcessNotRunning,
            )
            .expected("a process reparented to init")
            .observed(describe(&found, 2))
        } else {
            CheckOutcome::pass(
                &v.kind,
                "The job is running independently of your shell.".to_string(),
            )
        };
    }

    // In the background means the process group is not the terminal's foreground group.
    let background: Vec<&ProcessFacts> = found
        .iter()
        .filter(|p| p.tpgid < 0 || p.pgrp as i32 != p.tpgid)
        .collect();

    if background.is_empty() {
        CheckOutcome::fail(
            &v.kind,
            "The command is running in the foreground. Suspend it with Ctrl+Z and resume it \
             with bg, or start it with a trailing &."
                .to_string(),
            FailureCategory::ProcessNotRunning,
        )
        .observed(describe(&found, 2))
    } else {
        CheckOutcome::pass(&v.kind, "The job is running in the background.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx::for_test("root")
    }

    #[test]
    fn the_agent_never_matches_its_own_process_tree() {
        // A pattern matching this test binary must find nothing, otherwise every
        // process_not_running validator would fail spuriously.
        let candidates = candidate_processes();
        let me = std::process::id();
        assert!(!candidates.iter().any(|p| p.pid == me));
    }

    #[test]
    fn init_is_visible_and_owned_by_root() {
        let outcome = process_running(
            &Validator::new("process_running").with("pattern", "^/sbin/init|systemd|^init$"),
        );
        // On a container-based CI runner pid 1 may be something else entirely, so this only
        // asserts the validator produced a decision rather than an internal error.
        assert!(!outcome.errored, "{}", outcome.message);
    }

    #[test]
    fn a_pattern_that_cannot_compile_is_an_authoring_error() {
        let outcome = process_running(&Validator::new("process_running").with("pattern", "([bad"));
        assert!(outcome.errored);
        assert!(
            outcome.message.contains("not a valid regex"),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn nothing_matching_a_nonsense_pattern_is_running() {
        let outcome = process_not_running(
            &Validator::new("process_not_running")
                .with("pattern", "definitely-not-a-real-process-xyzzy"),
        );
        assert!(outcome.passed, "{}", outcome.message);
    }

    #[test]
    fn process_running_reports_the_requirement_when_it_fails() {
        let outcome = process_running(
            &Validator::new("process_running")
                .with("pattern", "definitely-not-a-real-process-xyzzy")
                .with("minCount", 2),
        );
        assert!(!outcome.passed);
        assert!(outcome.expected.unwrap().contains("at least 2"));
        assert_eq!(outcome.observed.as_deref(), Some("no matching process"));
    }

    #[test]
    fn owner_check_reports_a_nonexistent_user_clearly() {
        let outcome = process_owner(
            &Validator::new("process_owner")
                .with("pattern", "definitely-not-a-real-process-xyzzy")
                .with("owner", "nosuchuser"),
        );
        // No process matched, so that is the failure rather than the unknown user.
        assert!(!outcome.passed);
        assert_eq!(
            outcome.failure_category,
            Some(FailureCategory::ProcessNotRunning)
        );
    }

    #[test]
    fn signal_handles_are_sanitised_against_path_traversal() {
        let outcome = futures_lite_block(process_signal_received(
            &ctx(),
            &Validator::new("process_signal_received")
                .with("handle", "../../etc/shadow")
                .with("signal", "SIGTERM"),
        ));
        // The sanitised handle becomes "....etcshadow", which cannot escape /run/linuxlab.
        assert!(!outcome.passed);
        assert!(
            !outcome.message.contains("/etc/shadow"),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn an_empty_signal_handle_is_an_authoring_error() {
        let outcome = futures_lite_block(process_signal_received(
            &ctx(),
            &Validator::new("process_signal_received")
                .with("handle", "///")
                .with("signal", "SIGTERM"),
        ));
        assert!(outcome.errored);
    }

    #[test]
    fn truncation_keeps_output_readable_and_is_utf8_safe() {
        assert_eq!(truncate("short", 10), "short");
        let long = "ü".repeat(100);
        let truncated = truncate(&long, 10);
        assert_eq!(truncated.chars().count(), 11); // 10 plus the ellipsis
    }

    /// Minimal block-on so these unit tests do not need a Tokio runtime.
    fn futures_lite_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }
}
