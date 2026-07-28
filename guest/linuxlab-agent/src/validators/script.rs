//! Script and pipeline validators.
//!
//! `unit_test_passes` is the one that matters most. A pipeline lesson whose answer is only
//! checked against the visible sample data can be passed by printing the expected output, so
//! the learner's script is re-run against hidden fixtures with different names, different
//! ordering and extra noise lines (spec 14.2). Getting the visible case right is not enough.

use super::args;
use super::Ctx;
use crate::sys::{self, CommandOutput};
use shared_types::{CheckOutcome, FailureCategory, Validator};
use std::path::{Path, PathBuf};

pub async fn dispatch(ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "script_exists" => script_exists(validator),
        "script_executable" => script_executable(validator),
        "script_exit_code" => script_exit_code(ctx, validator).await,
        "stdout_exact" => stdout_exact(ctx, validator).await,
        "stdout_contains" => stdout_contains(ctx, validator).await,
        "stderr_contains" => stderr_contains(ctx, validator).await,
        "unit_test_passes" => unit_test_passes(ctx, validator).await,
        "shellcheck_passes" => shellcheck_passes(validator).await,
        "side_effect_exists" => return Some(side_effect_exists(ctx, validator).await),
        "idempotent_result" => idempotent_result(ctx, validator).await,
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

/// Fixture directories live under the lesson root and are not readable by the student, so a
/// learner cannot inspect the hidden cases their script will be graded against.
fn fixture_dir(ctx: &Ctx, validator: &Validator) -> Option<PathBuf> {
    let fixture = args::optional_string(validator, "fixture")?;
    let safe: String = fixture
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let safe = safe.trim_matches('.').to_string();
    if safe.is_empty() {
        return None;
    }
    Some(ctx.lesson_root().join("fixtures").join(safe))
}

fn script_exists(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let Some(facts) = sys::stat(&path) else {
        return CheckOutcome::fail(
            &v.kind,
            format!("{} does not exist.", path.display()),
            FailureCategory::WrongPath,
        );
    };
    if facts.kind != sys::FileKind::Regular {
        return CheckOutcome::fail(
            &v.kind,
            format!("{} is not a regular file.", path.display()),
            FailureCategory::WrongFileType,
        );
    }

    let first_line = std::fs::read(&path)
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .lines()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_default();

    if !first_line.starts_with("#!") {
        return CheckOutcome::fail(
            &v.kind,
            format!(
                "{} has no shebang line. Without one, the kernel does not know which interpreter \
                 to use.",
                path.display()
            ),
            FailureCategory::ScriptSyntaxFailure,
        )
        .expected("a first line starting with #!")
        .observed(if first_line.is_empty() {
            "an empty first line".to_string()
        } else {
            first_line.clone()
        });
    }

    if let Some(interpreter) = args::optional_string(v, "interpreter") {
        if !first_line.contains(interpreter) {
            return CheckOutcome::fail(
                &v.kind,
                format!("{} does not use {interpreter}.", path.display()),
                FailureCategory::ScriptSyntaxFailure,
            )
            .expected(interpreter)
            .observed(first_line);
        }
    }

    CheckOutcome::pass(
        &v.kind,
        format!("{} exists and has a shebang.", path.display()),
    )
}

fn script_executable(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let Some(facts) = sys::stat(&path) else {
        return CheckOutcome::fail(
            &v.kind,
            format!("{} does not exist.", path.display()),
            FailureCategory::WrongPath,
        );
    };

    // Which execute bit matters depends on who needs to run it, which is the point of the
    // lesson on chmod's user, group and other classes.
    let (needed_bit, description) = match args::optional_string(v, "forUser") {
        None => (0o111, "anyone".to_string()),
        Some(user) => match sys::read_passwd()
            .ok()
            .and_then(|entries| entries.into_iter().find(|e| e.name == user))
        {
            Some(entry) if entry.uid == facts.uid => (0o100, format!("its owner {user}")),
            Some(entry) => {
                let in_group = sys::groups_for_user(user)
                    .unwrap_or_default()
                    .iter()
                    .any(|g| sys::group_for_gid(facts.gid).as_deref() == Some(g.as_str()));
                let _ = entry;
                if in_group {
                    (0o010, format!("group members including {user}"))
                } else {
                    (0o001, format!("other users including {user}"))
                }
            }
            None => {
                return CheckOutcome::fail(
                    &v.kind,
                    format!("There is no account called {user}."),
                    FailureCategory::TaskPartiallyCompleted,
                )
            }
        },
    };

    if facts.permissions & needed_bit != 0 {
        CheckOutcome::pass(
            &v.kind,
            format!("{} is executable by {description}.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!(
                "{} is not executable by {description}. Add the execute bit with chmod.",
                path.display()
            ),
            FailureCategory::WrongPermissions,
        )
        .expected(format!("execute bit {}", sys::format_mode(needed_bit)))
        .observed(sys::format_mode(facts.permissions))
    }
}

async fn execute(
    ctx: &Ctx,
    v: &Validator,
    command: &str,
    working_dir: Option<&Path>,
) -> Result<CommandOutput, CheckOutcome> {
    sys::run_shell(
        command,
        Some(&ctx.subject_user),
        working_dir,
        args::timeout(v).or(Some(std::time::Duration::from_secs(20))),
    )
    .await
    .map_err(|err| CheckOutcome::error(&v.kind, err.to_string()))
}

fn timed_out(v: &Validator, output: &CommandOutput) -> Option<CheckOutcome> {
    output.timed_out.then(|| {
        CheckOutcome::fail(
            &v.kind,
            "The command did not finish in time. An unbounded loop or a command waiting for \
             input will do this."
                .to_string(),
            FailureCategory::ScriptLogicFailure,
        )
    })
}

async fn script_exit_code(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let Some(expected) = args::optional_integer(v, "exitCode") else {
        return CheckOutcome::error(&v.kind, "the exitCode parameter is missing or not a number");
    };
    let arguments = v
        .params
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str())
                .map(shell_quote)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    let working_dir = fixture_dir(ctx, v);
    let command = format!("{} {arguments}", shell_quote(&path.to_string_lossy()));

    let output = match args::optional_string(v, "stdin") {
        Some(stdin) => {
            let result = sys::run_with_stdin(
                "/bin/bash",
                &["-lc", &command],
                stdin,
                args::timeout(v).or(Some(std::time::Duration::from_secs(20))),
                working_dir.as_deref(),
            )
            .await;
            match result {
                Ok(output) => output,
                Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
            }
        }
        None => arg!(execute(ctx, v, &command, working_dir.as_deref()).await),
    };

    if let Some(outcome) = timed_out(v, &output) {
        return outcome;
    }

    if output.status as i64 == expected {
        CheckOutcome::pass(&v.kind, format!("The script exited with {expected}."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!(
                "The script exited with {} rather than {expected}. Exit codes are how other \
                 programs learn whether your script succeeded.",
                output.status
            ),
            FailureCategory::ScriptLogicFailure,
        )
        .expected(expected.to_string())
        .observed(output.status.to_string())
    }
}

/// Minimal single-quote shell escaping, so a path with a space cannot become two arguments.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

async fn stdout_exact(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let command = arg!(args::string(v, "command"));
    let expected = arg!(args::string(v, "expected"));
    let working_dir = fixture_dir(ctx, v);
    let output = arg!(execute(ctx, v, command, working_dir.as_deref()).await);
    if let Some(outcome) = timed_out(v, &output) {
        return outcome;
    }

    let trim = args::flag_or(v, "trimTrailingNewline", true);
    let actual = if trim {
        output.stdout.trim_end_matches('\n').to_string()
    } else {
        output.stdout.clone()
    };
    let expected_normalised = if trim {
        expected.trim_end_matches('\n').to_string()
    } else {
        expected.to_string()
    };

    if actual == expected_normalised {
        CheckOutcome::pass(&v.kind, "The output is exactly right.".to_string())
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The output is not what the task asked for. Build the pipeline up one stage at a \
             time and compare each stage."
                .to_string(),
            FailureCategory::PipelineOutputIncorrect,
        )
        .expected(first_lines(&expected_normalised, 6))
        .observed(first_lines(&actual, 6))
    }
}

fn first_lines(text: &str, limit: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return "no output".to_string();
    }
    let mut rendered = lines
        .iter()
        .take(limit)
        .copied()
        .collect::<Vec<_>>()
        .join(" / ");
    if lines.len() > limit {
        rendered.push_str(" …");
    }
    rendered
}

async fn stdout_contains(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let command = arg!(args::string(v, "command"));
    let needle = arg!(args::string(v, "text"));
    let working_dir = fixture_dir(ctx, v);
    let output = arg!(execute(ctx, v, command, working_dir.as_deref()).await);
    if let Some(outcome) = timed_out(v, &output) {
        return outcome;
    }
    if output.stdout.contains(needle) {
        CheckOutcome::pass(
            &v.kind,
            "The output contains what the task asked for.".to_string(),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The output does not contain what the task asked for.".to_string(),
            FailureCategory::PipelineOutputIncorrect,
        )
        .expected(needle)
        .observed(first_lines(&output.stdout, 6))
    }
}

async fn stderr_contains(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let command = arg!(args::string(v, "command"));
    let needle = arg!(args::string(v, "text"));
    let working_dir = fixture_dir(ctx, v);
    let output = arg!(execute(ctx, v, command, working_dir.as_deref()).await);
    if output.stderr.contains(needle) {
        CheckOutcome::pass(&v.kind, "The error output is as expected.".to_string())
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The error output does not contain what the task asked for. Remember that errors go \
             to a different stream from normal output."
                .to_string(),
            FailureCategory::IncorrectRedirect,
        )
        .expected(needle)
        .observed(first_lines(&output.stderr, 6))
    }
}

/// Runs the learner's script against every hidden case in a fixture directory.
///
/// A fixture is a directory of `case-*/` folders, each containing `input/`, an optional
/// `args` file and an `expected_stdout` file. The learner's script runs with `input/` as its
/// working directory, and every case must match.
async fn unit_test_passes(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let Some(fixture) = fixture_dir(ctx, v) else {
        return CheckOutcome::error(&v.kind, "the fixture parameter is missing or unusable");
    };
    if !fixture.is_dir() {
        return CheckOutcome::error(
            &v.kind,
            format!(
                "the fixture directory {} is not installed",
                fixture.display()
            ),
        );
    }
    if sys::stat(&path).is_none() {
        return CheckOutcome::fail(
            &v.kind,
            format!("{} does not exist.", path.display()),
            FailureCategory::WrongPath,
        );
    }

    let mut cases: Vec<PathBuf> = std::fs::read_dir(&fixture)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("case-"))
                })
                .collect()
        })
        .unwrap_or_default();
    cases.sort();

    if cases.is_empty() {
        return CheckOutcome::error(
            &v.kind,
            format!("the fixture {} contains no cases", fixture.display()),
        );
    }

    let mut failures = Vec::new();
    for case in &cases {
        let name = case
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let expected = std::fs::read_to_string(case.join("expected_stdout")).unwrap_or_default();
        let arguments = std::fs::read_to_string(case.join("args"))
            .map(|a| a.trim().to_string())
            .unwrap_or_default();
        let input_dir = case.join("input");
        let working_dir = if input_dir.is_dir() {
            input_dir
        } else {
            case.clone()
        };

        let command = format!("{} {arguments}", shell_quote(&path.to_string_lossy()));
        let output = match execute(ctx, v, &command, Some(&working_dir)).await {
            Ok(output) => output,
            Err(outcome) => return outcome,
        };
        if output.timed_out {
            failures.push(format!("{name}: did not finish in time"));
            continue;
        }
        if output.stdout.trim_end() != expected.trim_end() {
            failures.push(format!("{name}: output did not match"));
        }
    }

    if failures.is_empty() {
        CheckOutcome::pass(
            &v.kind,
            format!(
                "Your solution produced the right answer for all {} test cases, including the \
                 hidden ones.",
                cases.len()
            ),
        )
    } else {
        // The hidden inputs are never shown, only which cases failed. Otherwise the fixture
        // stops being hidden after one attempt.
        CheckOutcome::fail(
            &v.kind,
            format!(
                "Your solution worked for {} of {} test cases. The failing cases use different \
                 data from the example, so look for anything that assumes the sample values.",
                cases.len() - failures.len(),
                cases.len()
            ),
            FailureCategory::ScriptLogicFailure,
        )
        .observed(failures.join("; "))
    }
}

async fn shellcheck_passes(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let severity = args::optional_string(v, "severity").unwrap_or("warning");
    let rendered = path.to_string_lossy().to_string();

    let excludes = v
        .params
        .get("exclude")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();

    let mut arguments: Vec<&str> = vec!["--format=gcc", "--severity", severity];
    if !excludes.is_empty() {
        arguments.push("--exclude");
        arguments.push(&excludes);
    }
    arguments.push("--");
    arguments.push(&rendered);

    let output = match sys::run("shellcheck", &arguments, None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    if output.success() {
        CheckOutcome::pass(
            &v.kind,
            format!("shellcheck reports no problems at or above {severity}."),
        )
    } else {
        let findings: Vec<&str> = output.stdout.lines().take(5).collect();
        CheckOutcome::fail(
            &v.kind,
            "shellcheck found problems in the script.".to_string(),
            FailureCategory::ScriptSyntaxFailure,
        )
        .observed(findings.join(" / "))
    }
}

/// Runs a command, then applies nested validators to whatever state it produced.
async fn side_effect_exists(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let command = match args::string(v, "command") {
        Ok(command) => command,
        Err(outcome) => return outcome,
    };
    // This validator is the authoring harness's stand-in for a learner typing in the
    // terminal. The serial getty starts in that learner's home directory, while the agent
    // service itself starts at `/`; using the service cwd would make every valid relative
    // solution fail for reasons a learner can never encounter.
    let working_dir = if ctx.subject_user == "root" {
        PathBuf::from("/root")
    } else {
        PathBuf::from("/home").join(&ctx.subject_user)
    };
    let output = match execute(ctx, v, command, Some(&working_dir)).await {
        Ok(output) => output,
        Err(outcome) => return outcome,
    };
    if output.timed_out {
        return CheckOutcome::fail(
            &v.kind,
            "The command did not finish in time.".to_string(),
            FailureCategory::ScriptLogicFailure,
        );
    }

    let Some(nested_values) = v.params.get("then").and_then(|value| value.as_array()) else {
        return CheckOutcome::error(&v.kind, "the then parameter must be an array of validators");
    };

    let mut failed = Vec::new();
    for value in nested_values {
        let nested: Validator = match serde_json::from_value(value.clone()) {
            Ok(nested) => nested,
            Err(err) => {
                return CheckOutcome::error(
                    &v.kind,
                    format!("a nested validator is malformed: {err}"),
                )
            }
        };
        // Nested validators go back through the top-level dispatcher, which re-checks them
        // against the registry first.
        let outcome = Box::pin(super::evaluate(ctx, &nested)).await;
        if !outcome.passed {
            failed.push(outcome.message);
        }
    }

    if failed.is_empty() {
        CheckOutcome::pass(
            &v.kind,
            "Running the command produced the expected result.".to_string(),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The command ran, but it did not have the effect the task asked for.".to_string(),
            FailureCategory::ScriptLogicFailure,
        )
        .observed(failed.join("; "))
    }
}

/// Runs a command several times and requires the same result each time.
async fn idempotent_result(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let command = arg!(args::string(v, "command"));
    let runs = args::optional_integer(v, "runs").unwrap_or(2).clamp(2, 5);

    let mut first: Option<(i32, String)> = None;
    for attempt in 1..=runs {
        let output = arg!(execute(ctx, v, command, None).await);
        if output.timed_out {
            return CheckOutcome::fail(
                &v.kind,
                format!("Run {attempt} did not finish in time."),
                FailureCategory::ScriptLogicFailure,
            );
        }
        let result = (output.status, output.stdout.clone());
        match &first {
            None => first = Some(result),
            Some(expected) if *expected != result => {
                return CheckOutcome::fail(
                    &v.kind,
                    format!(
                        "Run {attempt} produced a different result from the first run. A script \
                         that is safe to re-run should reach the same state every time."
                    ),
                    FailureCategory::ScriptLogicFailure,
                )
                .expected(format!("exit {} with the same output", expected.0))
                .observed(format!("exit {} with different output", result.0));
            }
            _ => {}
        }
    }

    CheckOutcome::pass(
        &v.kind,
        format!("Running the command {runs} times produced the same result each time."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quoting_survives_spaces_and_quotes() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("with space"), "'with space'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn script_without_a_shebang_is_reported_as_a_syntax_problem() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.sh");
        std::fs::write(&path, b"echo hello\n").unwrap();

        let outcome =
            script_exists(&Validator::new("script_exists").with("path", path.to_str().unwrap()));
        assert!(!outcome.passed);
        assert_eq!(
            outcome.failure_category,
            Some(FailureCategory::ScriptSyntaxFailure)
        );
        assert!(outcome.message.contains("shebang"), "{}", outcome.message);
    }

    #[test]
    fn script_with_a_shebang_passes_and_the_interpreter_can_be_required() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.sh");
        std::fs::write(&path, b"#!/usr/bin/env bash\necho hello\n").unwrap();

        let ok =
            script_exists(&Validator::new("script_exists").with("path", path.to_str().unwrap()));
        assert!(ok.passed, "{}", ok.message);

        let requires_bash = script_exists(
            &Validator::new("script_exists")
                .with("path", path.to_str().unwrap())
                .with("interpreter", "bash"),
        );
        assert!(requires_bash.passed);

        let requires_python = script_exists(
            &Validator::new("script_exists")
                .with("path", path.to_str().unwrap())
                .with("interpreter", "python3"),
        );
        assert!(!requires_python.passed);
    }

    #[test]
    fn executability_is_checked_against_the_actual_bits() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.sh");
        std::fs::write(&path, b"#!/bin/bash\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let before = script_executable(
            &Validator::new("script_executable").with("path", path.to_str().unwrap()),
        );
        assert!(!before.passed);
        assert!(before.message.contains("chmod"), "{}", before.message);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let after = script_executable(
            &Validator::new("script_executable").with("path", path.to_str().unwrap()),
        );
        assert!(after.passed, "{}", after.message);
    }

    #[test]
    fn fixture_paths_cannot_escape_the_lesson_root() {
        let ctx = Ctx::for_test("student");
        let validator = Validator::new("unit_test_passes")
            .with("path", "/home/student/x.sh")
            .with("fixture", "../../../etc");
        let resolved = fixture_dir(&ctx, &validator).unwrap();
        let rendered = resolved.to_string_lossy();
        assert!(!rendered.contains(".."), "{rendered}");
        assert!(rendered.starts_with(&*ctx.lesson_root().to_string_lossy()));
    }

    #[test]
    fn output_previews_are_truncated_and_labelled() {
        assert_eq!(first_lines("", 3), "no output");
        assert_eq!(first_lines("a\nb", 3), "a / b");
        assert!(first_lines("a\nb\nc\nd", 3).ends_with('…'));
    }
}
