//! systemd, journal and listening-port validators.

use super::args;
use super::Ctx;
use crate::sys;
use shared_types::{CheckOutcome, FailureCategory, Validator};

pub async fn dispatch(ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "service_active" => service_state(validator, "is-active", "active", true).await,
        "service_inactive" => service_state(validator, "is-active", "active", false).await,
        "service_enabled" => service_state(validator, "is-enabled", "enabled", true).await,
        "service_disabled" => service_state(validator, "is-enabled", "enabled", false).await,
        "service_failed" => service_failed(validator).await,
        "unit_file_valid" => unit_file_valid(validator).await,
        "journal_contains" => journal_contains(ctx, validator).await,
        "port_listening" => port_listening(validator),
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

/// Rejects unit names that could smuggle extra arguments into systemctl.
fn safe_unit(validator: &Validator) -> Result<String, CheckOutcome> {
    let unit = args::string(validator, "unit")?;
    let acceptable = !unit.is_empty()
        && !unit.starts_with('-')
        && unit
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@' | ':' | '\\'));
    if acceptable {
        Ok(unit.to_string())
    } else {
        Err(CheckOutcome::error(
            &validator.kind,
            format!("'{unit}' is not a valid systemd unit name"),
        ))
    }
}

/// `systemctl is-active` and `is-enabled` exit non-zero for the negative case and print the
/// state either way, so the printed word is what we compare rather than the exit status.
async fn service_state(
    v: &Validator,
    subcommand: &str,
    positive: &str,
    want_positive: bool,
) -> CheckOutcome {
    let unit = arg!(safe_unit(v));
    let output = match sys::run("systemctl", &[subcommand, "--", &unit], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if output.timed_out {
        return CheckOutcome::error(&v.kind, format!("systemctl {subcommand} timed out"));
    }
    let state = output.stdout_trimmed();
    let state = if state.is_empty() {
        output.stderr.trim()
    } else {
        state
    };
    let is_positive = state == positive;

    if is_positive == want_positive {
        CheckOutcome::pass(&v.kind, format!("{unit} is {state}.")).observed(state)
    } else {
        let expectation = if want_positive {
            positive.to_string()
        } else {
            format!("not {positive}")
        };
        CheckOutcome::fail(
            &v.kind,
            format!(
                "{unit} is {state}. Check `systemctl status {unit}` and `journalctl -u {unit}`."
            ),
            FailureCategory::ServiceNotActive,
        )
        .expected(expectation)
        .observed(state)
    }
}

async fn service_failed(v: &Validator) -> CheckOutcome {
    let unit = arg!(safe_unit(v));
    let output = match sys::run(
        "systemctl",
        &["show", "-p", "ActiveState", "--value", "--", &unit],
        None,
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    let state = output.stdout_trimmed().to_string();
    if state == "failed" {
        CheckOutcome::pass(&v.kind, format!("{unit} is in the failed state."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{unit} is {state}, not failed."),
            FailureCategory::ServiceNotActive,
        )
        .expected("failed")
        .observed(state)
    }
}

async fn unit_file_valid(v: &Validator) -> CheckOutcome {
    let unit = arg!(safe_unit(v));
    let output = match sys::run("systemd-analyze", &["verify", "--", &unit], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    // systemd-analyze verify writes complaints to stderr and can still exit zero, so the
    // absence of output is the real signal.
    let complaints: Vec<&str> = output
        .stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if output.success() && complaints.is_empty() {
        CheckOutcome::pass(&v.kind, format!("The unit file for {unit} is valid."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("The unit file for {unit} has problems."),
            FailureCategory::ScriptSyntaxFailure,
        )
        .observed(complaints.join("; "))
    }
}

async fn journal_contains(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let text = arg!(args::string(v, "text"));
    // Default to the start of this attempt so a message left by a previous try cannot make a
    // later attempt pass.
    let since = args::optional_string(v, "since")
        .map(|s| s.to_string())
        .unwrap_or_else(|| ctx.attempt_since_expression());

    let mut command: Vec<String> = vec![
        "--no-pager".into(),
        "--output=cat".into(),
        "--since".into(),
        since.clone(),
    ];
    if let Some(unit) = args::optional_string(v, "unit") {
        command.push("-u".into());
        command.push(unit.to_string());
    }
    if let Some(priority) = args::optional_string(v, "priority") {
        command.push("-p".into());
        command.push(priority.to_string());
    }

    let borrowed: Vec<&str> = command.iter().map(|s| s.as_str()).collect();
    let output = match sys::run("journalctl", &borrowed, None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    if output.stdout.contains(text) {
        CheckOutcome::pass(
            &v.kind,
            "The journal contains the expected message.".to_string(),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The journal does not contain the expected message.".to_string(),
            FailureCategory::ServiceNotActive,
        )
        .expected(text)
        .observed(format!(
            "{} journal lines since {since}",
            output.stdout.lines().count()
        ))
    }
}

fn port_listening(v: &Validator) -> CheckOutcome {
    let Some(port) = args::optional_integer(v, "port") else {
        return CheckOutcome::error(&v.kind, "the port parameter is missing or not a number");
    };
    let protocol = args::optional_string(v, "protocol").unwrap_or("tcp");
    let sockets = sys::read_sockets(protocol);

    let listening: Vec<_> = sockets
        .iter()
        .filter(|s| s.port as i64 == port && s.listening)
        .collect();

    if listening.is_empty() {
        let other_ports: Vec<String> = {
            let mut ports: Vec<u16> = sockets
                .iter()
                .filter(|s| s.listening)
                .map(|s| s.port)
                .collect();
            ports.sort_unstable();
            ports.dedup();
            ports.iter().take(8).map(|p| p.to_string()).collect()
        };
        return CheckOutcome::fail(
            &v.kind,
            format!("Nothing is listening on {protocol} port {port}."),
            FailureCategory::WrongPort,
        )
        .expected(format!("{protocol}/{port}"))
        .observed(if other_ports.is_empty() {
            "no listening sockets".to_string()
        } else {
            format!("listening on {}", other_ports.join(", "))
        });
    }

    if let Some(expected_address) = args::optional_string(v, "address") {
        // Binding to 127.0.0.1 when the task needs 0.0.0.0 is one of the most common
        // "the service is up but unreachable" faults, so it is called out specifically.
        let addresses: Vec<&str> = listening.iter().map(|s| s.local_address.as_str()).collect();
        if !addresses.contains(&expected_address) {
            return CheckOutcome::fail(
                &v.kind,
                format!(
                    "Port {port} is open but bound to the wrong address, so it is not reachable \
                     where the task expects."
                ),
                FailureCategory::WrongPort,
            )
            .expected(expected_address)
            .observed(addresses.join(", "));
        }
    }

    CheckOutcome::pass(
        &v.kind,
        format!("Something is listening on {protocol} port {port}."),
    )
    .observed(
        listening
            .iter()
            .map(|s| format!("{}:{}", s.local_address, s.port))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_names_that_could_inject_flags_are_rejected() {
        for bad in ["--all", "-h", "nginx; rm -rf /", "nginx service", ""] {
            let v = Validator::new("service_active").with("unit", bad);
            assert!(safe_unit(&v).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn ordinary_and_templated_unit_names_are_accepted() {
        for good in [
            "nginx.service",
            "report-api",
            "getty@tty1.service",
            "my_unit.timer",
        ] {
            let v = Validator::new("service_active").with("unit", good);
            assert_eq!(safe_unit(&v).unwrap(), good);
        }
    }

    #[test]
    fn a_closed_port_is_reported_with_what_is_actually_listening() {
        // Port 0 can never be listening, so this exercises the failure path deterministically.
        let outcome = port_listening(&Validator::new("port_listening").with("port", 0));
        assert!(!outcome.passed);
        assert_eq!(outcome.failure_category, Some(FailureCategory::WrongPort));
        assert!(outcome.observed.is_some());
    }

    #[test]
    fn a_missing_port_parameter_is_an_internal_error() {
        let outcome = port_listening(&Validator::new("port_listening"));
        assert!(outcome.errored);
    }

    #[test]
    fn protocol_defaults_to_tcp() {
        let outcome = port_listening(&Validator::new("port_listening").with("port", 0));
        assert!(outcome.message.contains("tcp"), "{}", outcome.message);
    }
}
