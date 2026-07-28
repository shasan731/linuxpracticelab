//! Networking validators.
//!
//! All of these run inside the guest, and in internal-lab lessons inside a specific network
//! namespace. Nothing here can reach the learner's physical network: the guest has no host
//! interface at all in offline and internal-lab modes.

use super::args;
use super::Ctx;
use crate::sys::{self, CommandOutput};
use shared_types::{CheckOutcome, FailureCategory, Validator};
use std::time::Duration;

pub async fn dispatch(ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "interface_exists" => interface_exists(ctx, validator).await,
        "interface_state" => interface_state(ctx, validator).await,
        "ip_address" => ip_address(ctx, validator).await,
        "route_exists" => route_exists(ctx, validator).await,
        "default_route" => default_route(ctx, validator).await,
        "dns_resolution" => dns_resolution(ctx, validator).await,
        "tcp_connection" => tcp_connection(ctx, validator).await,
        "udp_listener" => udp_listener(validator),
        "http_status" => http_status(ctx, validator).await,
        "http_body" => http_body(ctx, validator).await,
        "firewall_rule" => firewall_rule(ctx, validator).await,
        "ssh_key_valid" => ssh_key_valid(validator).await,
        "remote_file_exists" => remote_file_exists(ctx, validator).await,
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

/// Runs a command in the validator's namespace when one is set, otherwise in the guest's
/// default namespace.
async fn run(
    ctx: &Ctx,
    validator: &Validator,
    program: &str,
    arguments: &[&str],
    timeout: Option<Duration>,
) -> anyhow::Result<CommandOutput> {
    match ctx.namespace_for(validator) {
        Some(namespace) => sys::run_in_namespace(&namespace, program, arguments, timeout).await,
        None => sys::run(program, arguments, timeout).await,
    }
}

fn unreachable(v: &Validator, message: String) -> CheckOutcome {
    CheckOutcome::fail(&v.kind, message, FailureCategory::NetworkUnreachable)
}

async fn interface_exists(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let interface = arg!(args::string(v, "interface"));
    let output = match run(ctx, v, "ip", &["-o", "link", "show"], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    let names = parse_interface_names(&output.stdout);
    if names.iter().any(|n| n == interface) {
        CheckOutcome::pass(&v.kind, format!("The interface {interface} exists."))
    } else {
        unreachable(v, format!("There is no interface called {interface}."))
            .expected(interface)
            .observed(names.join(", "))
    }
}

/// `ip -o link show` prints `2: eth0@if3: <BROADCAST,...> mtu 1500 ...`.
pub fn parse_interface_names(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let after_index = line.split_once(':')?.1.trim_start();
            let name = after_index.split(':').next()?.trim();
            // Strip the @parent suffix that veth and vlan interfaces carry.
            Some(name.split('@').next()?.to_string())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

async fn interface_state(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let interface = arg!(args::string(v, "interface"));
    let expected = arg!(args::string(v, "state"));
    let output = match run(
        ctx,
        v,
        "ip",
        &["-o", "link", "show", "dev", interface],
        None,
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if !output.success() {
        return unreachable(v, format!("There is no interface called {interface}."));
    }

    // The operational state is what matters to a learner: an interface can be
    // administratively UP while its carrier is down.
    let operational = parse_oper_state(&output.stdout).unwrap_or_else(|| "unknown".to_string());
    let is_up = operational.eq_ignore_ascii_case("up")
        || (operational.eq_ignore_ascii_case("unknown")
            && output.stdout.contains("state UNKNOWN")
            && output.stdout.contains(",UP"));

    let matches = (expected == "up" && is_up) || (expected == "down" && !is_up);
    if matches {
        CheckOutcome::pass(&v.kind, format!("{interface} is {operational}."))
    } else {
        unreachable(
            v,
            format!("{interface} is {operational}, not {expected}. Bring it up with `ip link set {interface} up`."),
        )
        .expected(expected)
        .observed(operational)
    }
}

pub fn parse_oper_state(output: &str) -> Option<String> {
    let tokens: Vec<&str> = output.split_whitespace().collect();
    tokens
        .iter()
        .position(|t| *t == "state")
        .and_then(|index| tokens.get(index + 1))
        .map(|state| state.to_lowercase())
}

async fn ip_address(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let interface = arg!(args::string(v, "interface"));
    let expected = arg!(args::string(v, "address"));
    let output = match run(
        ctx,
        v,
        "ip",
        &["-o", "address", "show", "dev", interface],
        None,
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if !output.success() {
        return unreachable(v, format!("There is no interface called {interface}."));
    }
    let addresses = parse_addresses(&output.stdout);
    if addresses.iter().any(|a| a == expected) {
        CheckOutcome::pass(&v.kind, format!("{interface} has the address {expected}."))
    } else {
        unreachable(
            v,
            format!("{interface} does not have the address {expected}."),
        )
        .expected(expected)
        .observed(if addresses.is_empty() {
            "no addresses".to_string()
        } else {
            addresses.join(", ")
        })
    }
}

/// `ip -o address show` prints `2: eth0    inet 10.20.0.10/24 brd ... scope global eth0`.
pub fn parse_addresses(output: &str) -> Vec<String> {
    output
        .lines()
        .flat_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            tokens
                .iter()
                .enumerate()
                .filter(|(_, token)| **token == "inet" || **token == "inet6")
                .filter_map(|(index, _)| tokens.get(index + 1).map(|a| a.to_string()))
                .collect::<Vec<_>>()
        })
        .collect()
}

async fn route_exists(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let destination = arg!(args::string(v, "destination"));
    let output = match run(ctx, v, "ip", &["route", "show"], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    let matching: Vec<&str> = output
        .stdout
        .lines()
        .filter(|line| line.split_whitespace().next() == Some(destination))
        .collect();

    if matching.is_empty() {
        return unreachable(v, format!("There is no route to {destination}."))
            .expected(destination)
            .observed(summarise_routes(&output.stdout));
    }
    if let Some(via) = args::optional_string(v, "via") {
        if !matching
            .iter()
            .any(|line| line.contains(&format!("via {via}")))
        {
            return unreachable(
                v,
                format!("The route to {destination} does not go via {via}."),
            )
            .expected(format!("via {via}"))
            .observed(matching.join("; "));
        }
    }
    if let Some(dev) = args::optional_string(v, "dev") {
        if !matching
            .iter()
            .any(|line| line.contains(&format!("dev {dev}")))
        {
            return unreachable(v, format!("The route to {destination} does not use {dev}."))
                .expected(format!("dev {dev}"))
                .observed(matching.join("; "));
        }
    }
    CheckOutcome::pass(&v.kind, format!("A route to {destination} exists."))
}

fn summarise_routes(output: &str) -> String {
    let routes: Vec<&str> = output.lines().take(5).collect();
    if routes.is_empty() {
        "an empty routing table".to_string()
    } else {
        routes.join("; ")
    }
}

async fn default_route(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let output = match run(ctx, v, "ip", &["route", "show", "default"], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    let line = output.stdout_trimmed().to_string();
    if line.is_empty() {
        return unreachable(
            v,
            "There is no default route, so traffic to other networks has nowhere to go."
                .to_string(),
        );
    }
    if let Some(via) = args::optional_string(v, "via") {
        if !line.contains(&format!("via {via}")) {
            return unreachable(v, format!("The default route does not go via {via}."))
                .expected(format!("via {via}"))
                .observed(&line);
        }
    }
    if let Some(dev) = args::optional_string(v, "dev") {
        if !line.contains(&format!("dev {dev}")) {
            return unreachable(v, format!("The default route does not use {dev}."))
                .expected(format!("dev {dev}"))
                .observed(&line);
        }
    }
    CheckOutcome::pass(&v.kind, "A default route is configured.".to_string()).observed(line)
}

async fn dns_resolution(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let name = arg!(args::string(v, "name"));
    let mut arguments: Vec<String> = vec!["+short".into(), "+time=2".into(), "+tries=1".into()];
    if let Some(server) = args::optional_string(v, "server") {
        arguments.push(format!("@{server}"));
    }
    arguments.push(name.to_string());
    let borrowed: Vec<&str> = arguments.iter().map(|s| s.as_str()).collect();

    let output = match run(ctx, v, "dig", &borrowed, Some(Duration::from_secs(6))).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    let answers: Vec<&str> = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    if answers.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            format!("{name} does not resolve. Check /etc/resolv.conf and the DNS server."),
            FailureCategory::DnsFailure,
        )
        .expected(format!("an address for {name}"))
        .observed("no answer");
    }
    if let Some(expected) = args::optional_string(v, "expect") {
        if !answers.iter().any(|a| *a == expected) {
            return CheckOutcome::fail(
                &v.kind,
                format!("{name} resolves, but not to the expected address."),
                FailureCategory::DnsFailure,
            )
            .expected(expected)
            .observed(answers.join(", "));
        }
    }
    CheckOutcome::pass(&v.kind, format!("{name} resolves.")).observed(answers.join(", "))
}

async fn tcp_connection(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let host = arg!(args::string(v, "host"));
    let Some(port) = args::optional_integer(v, "port") else {
        return CheckOutcome::error(&v.kind, "the port parameter is missing or not a number");
    };
    let timeout = args::optional_integer(v, "timeoutMs").unwrap_or(3000);
    let seconds = ((timeout as f64 / 1000.0).ceil() as u64).clamp(1, 30);
    let port_text = port.to_string();
    let wait = seconds.to_string();

    // -z scans without sending data, so nothing the server sees changes state.
    let output = match run(
        ctx,
        v,
        "nc",
        &["-z", "-w", &wait, host, &port_text],
        Some(Duration::from_secs(seconds + 2)),
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    if output.success() {
        CheckOutcome::pass(&v.kind, format!("A TCP connection to {host}:{port} works."))
    } else {
        unreachable(
            v,
            format!(
                "Nothing accepted a TCP connection on {host}:{port}. Check the service, the bind \
                 address and the firewall in that order."
            ),
        )
    }
}

fn udp_listener(v: &Validator) -> CheckOutcome {
    let Some(port) = args::optional_integer(v, "port") else {
        return CheckOutcome::error(&v.kind, "the port parameter is missing or not a number");
    };
    let sockets = sys::read_sockets("udp");
    let bound: Vec<_> = sockets.iter().filter(|s| s.port as i64 == port).collect();
    if bound.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            format!("Nothing is bound to UDP port {port}."),
            FailureCategory::WrongPort,
        );
    }
    if let Some(expected) = args::optional_string(v, "address") {
        let addresses: Vec<&str> = bound.iter().map(|s| s.local_address.as_str()).collect();
        if !addresses.contains(&expected) {
            return CheckOutcome::fail(
                &v.kind,
                format!("UDP port {port} is bound to the wrong address."),
                FailureCategory::WrongPort,
            )
            .expected(expected)
            .observed(addresses.join(", "));
        }
    }
    CheckOutcome::pass(&v.kind, format!("A UDP socket is bound to port {port}."))
}

async fn curl(
    ctx: &Ctx,
    v: &Validator,
    url: &str,
    extra: &[&str],
) -> Result<CommandOutput, CheckOutcome> {
    let timeout = args::optional_integer(v, "timeoutMs").unwrap_or(5000);
    let seconds = ((timeout as f64 / 1000.0).ceil() as u64).clamp(1, 30);
    let wait = seconds.to_string();
    let mut arguments: Vec<&str> = vec!["--silent", "--show-error", "--max-time", &wait];
    arguments.extend_from_slice(extra);
    arguments.push("--");
    arguments.push(url);

    run(
        ctx,
        v,
        "curl",
        &arguments,
        Some(Duration::from_secs(seconds + 2)),
    )
    .await
    .map_err(|err| CheckOutcome::error(&v.kind, err.to_string()))
}

async fn http_status(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let url = arg!(args::string(v, "url"));
    let Some(expected) = args::optional_integer(v, "status") else {
        return CheckOutcome::error(&v.kind, "the status parameter is missing or not a number");
    };
    let method = args::optional_string(v, "method").unwrap_or("GET");
    let mut extra: Vec<&str> = vec!["-o", "/dev/null", "-w", "%{http_code}"];
    match method {
        "HEAD" => extra.push("--head"),
        "POST" => {
            extra.push("-X");
            extra.push("POST");
        }
        _ => {}
    }

    let output = arg!(curl(ctx, v, url, &extra).await);
    if !output.success() && output.stdout_trimmed().is_empty() {
        return unreachable(
            v,
            format!(
                "{url} could not be reached at all ({}).",
                output.stderr.trim()
            ),
        );
    }
    let actual = output.stdout_trimmed();
    if actual == expected.to_string() {
        CheckOutcome::pass(&v.kind, format!("{url} returned {actual}."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{url} returned {actual} rather than {expected}."),
            FailureCategory::NetworkUnreachable,
        )
        .expected(expected.to_string())
        .observed(actual)
    }
}

async fn http_body(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let url = arg!(args::string(v, "url"));
    let needle = arg!(args::string(v, "contains"));
    let output = arg!(curl(ctx, v, url, &[]).await);
    if !output.success() && output.stdout.is_empty() {
        return unreachable(
            v,
            format!("{url} could not be reached ({}).", output.stderr.trim()),
        );
    }
    if output.stdout.contains(needle) {
        CheckOutcome::pass(&v.kind, format!("{url} returned the expected content."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{url} did not return the expected content."),
            FailureCategory::NetworkUnreachable,
        )
        .expected(needle)
    }
}

async fn firewall_rule(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let needle = arg!(args::string(v, "contains"));
    let output = match run(ctx, v, "nft", &["list", "ruleset"], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if !output.success() {
        return CheckOutcome::error(
            &v.kind,
            format!(
                "could not read the firewall ruleset: {}",
                output.stderr.trim()
            ),
        );
    }

    // Narrow to a table or chain block when the author named one, so a rule in the wrong
    // chain does not count.
    let scoped = match (
        args::optional_string(v, "table"),
        args::optional_string(v, "chain"),
    ) {
        (None, None) => output.stdout.clone(),
        (table, chain) => extract_block(&output.stdout, table, chain),
    };

    if scoped.contains(needle) {
        CheckOutcome::pass(
            &v.kind,
            "The firewall ruleset contains the expected rule.".to_string(),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The firewall ruleset does not contain the expected rule.".to_string(),
            FailureCategory::NetworkUnreachable,
        )
        .expected(needle)
    }
}

/// Extracts the text of a named `table` and/or `chain` block from `nft list ruleset`.
pub fn extract_block(ruleset: &str, table: Option<&str>, chain: Option<&str>) -> String {
    let mut collected = String::new();
    let mut depth = 0usize;
    let mut in_table = table.is_none();
    let mut in_chain = chain.is_none();

    for line in ruleset.lines() {
        let trimmed = line.trim();
        if let Some(name) = table {
            if trimmed.starts_with("table ") && trimmed.contains(name) {
                in_table = true;
            } else if trimmed.starts_with("table ") {
                in_table = false;
                in_chain = chain.is_none();
            }
        }
        if let Some(name) = chain {
            if trimmed.starts_with("chain ") {
                in_chain = trimmed
                    .split_whitespace()
                    .nth(1)
                    .map(|c| c == name)
                    .unwrap_or(false);
            }
        }
        depth = depth
            .saturating_add(trimmed.matches('{').count())
            .saturating_sub(trimmed.matches('}').count());
        if in_table && in_chain {
            collected.push_str(line);
            collected.push('\n');
        }
        if depth == 0 && trimmed.starts_with('}') {
            in_chain = chain.is_none();
        }
    }
    collected
}

async fn ssh_key_valid(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let rendered = path.to_string_lossy().to_string();

    let output = match sys::run("ssh-keygen", &["-l", "-f", &rendered], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if !output.success() {
        return CheckOutcome::fail(
            &v.kind,
            format!("{rendered} is not a valid SSH key."),
            FailureCategory::PermissionDenied,
        )
        .observed(output.stderr.trim());
    }

    // ssh-keygen -l prints "3072 SHA256:... comment (RSA)".
    let summary = output.stdout_trimmed().to_string();
    if let Some(expected_type) = args::optional_string(v, "keyType") {
        let matches = summary
            .to_lowercase()
            .contains(&format!("({})", expected_type.to_lowercase()));
        if !matches {
            return CheckOutcome::fail(
                &v.kind,
                format!("{rendered} is not an {expected_type} key."),
                FailureCategory::PermissionDenied,
            )
            .expected(expected_type)
            .observed(summary);
        }
    }

    if args::flag(v, "requireSafeMode") {
        let Some(facts) = sys::stat(&path) else {
            return CheckOutcome::fail(
                &v.kind,
                format!("{rendered} could not be inspected."),
                FailureCategory::WrongPath,
            );
        };
        // OpenSSH itself refuses to use a private key that others can read.
        if facts.permissions & 0o077 != 0 {
            return CheckOutcome::fail(
                &v.kind,
                format!(
                    "{rendered} is readable by other users, so ssh will refuse to use it. \
                     A private key should be 0600."
                ),
                FailureCategory::WrongPermissions,
            )
            .expected("0600")
            .observed(sys::format_mode(facts.permissions));
        }
    }

    CheckOutcome::pass(&v.kind, format!("{rendered} is a valid SSH key.")).observed(summary)
}

async fn remote_file_exists(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let host = arg!(args::string(v, "host"));
    let path = arg!(args::string(v, "path"));
    let user = args::optional_string(v, "user").unwrap_or("student");
    let target = format!("{user}@{host}");

    // BatchMode means a missing key fails immediately instead of hanging on a prompt.
    let output = match run(
        ctx,
        v,
        "ssh",
        &[
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=4",
            &target,
            "test",
            "-e",
            path,
        ],
        Some(Duration::from_secs(12)),
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    if output.success() {
        CheckOutcome::pass(&v.kind, format!("{path} exists on {host}."))
    } else if output.stderr.contains("Permission denied") {
        CheckOutcome::fail(
            &v.kind,
            format!("Could not log in to {host} as {user} without a password."),
            FailureCategory::PermissionDenied,
        )
    } else {
        unreachable(
            v,
            format!("{path} was not found on {host}, or {host} could not be reached."),
        )
        .observed(output.stderr.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_names_are_parsed_including_veth_suffixes() {
        let output = "1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN\n\
                      2: eth0@if7: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 state UP\n";
        assert_eq!(parse_interface_names(output), vec!["lo", "eth0"]);
    }

    #[test]
    fn operational_state_is_read_from_the_state_field() {
        let up = "2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc noqueue state UP mode DEFAULT";
        assert_eq!(parse_oper_state(up).as_deref(), Some("up"));
        let down = "2: eth0: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN mode DEFAULT";
        assert_eq!(parse_oper_state(down).as_deref(), Some("down"));
    }

    #[test]
    fn addresses_are_parsed_for_both_families() {
        let output = "2: eth0    inet 10.20.0.10/24 brd 10.20.0.255 scope global eth0\n\
                      2: eth0    inet6 fe80::1/64 scope link\n";
        assert_eq!(parse_addresses(output), vec!["10.20.0.10/24", "fe80::1/64"]);
    }

    #[test]
    fn an_interface_with_no_address_yields_an_empty_list() {
        assert!(parse_addresses("2: eth0    <no addresses>\n").is_empty());
    }

    #[test]
    fn firewall_scoping_narrows_to_the_named_chain() {
        let ruleset = "table inet filter {\n\
                       \tchain input {\n\
                       \t\ttype filter hook input priority 0; policy drop;\n\
                       \t\ttcp dport 22 accept\n\
                       \t}\n\
                       \tchain output {\n\
                       \t\ttcp dport 8080 accept\n\
                       \t}\n\
                       }\n";
        let input_chain = extract_block(ruleset, Some("filter"), Some("input"));
        assert!(input_chain.contains("tcp dport 22 accept"));
        assert!(
            !input_chain.contains("tcp dport 8080 accept"),
            "a rule in another chain must not count: {input_chain}"
        );
    }

    #[test]
    fn unscoped_firewall_matching_sees_the_whole_ruleset() {
        let ruleset = "table inet filter {\n\tchain input {\n\t\ttcp dport 22 accept\n\t}\n}\n";
        assert!(extract_block(ruleset, None, None).contains("dport 22"));
    }

    #[test]
    fn a_missing_port_is_an_internal_error_not_a_learner_failure() {
        let outcome = udp_listener(&Validator::new("udp_listener"));
        assert!(outcome.errored);
    }

    #[test]
    fn udp_port_zero_is_never_bound() {
        let outcome = udp_listener(&Validator::new("udp_listener").with("port", 0));
        assert!(!outcome.passed);
        assert!(!outcome.errored);
    }
}
