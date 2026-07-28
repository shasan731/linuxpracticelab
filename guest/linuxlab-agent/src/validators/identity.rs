//! User, group, sudo and ACL validators.

use super::args;
use super::Ctx;
use crate::sys;
use shared_types::{CheckOutcome, FailureCategory, Validator};

pub async fn dispatch(_ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "user_exists" => user_exists(validator),
        "user_missing" => user_missing(validator),
        "group_exists" => group_exists(validator),
        "group_membership" => group_membership(validator),
        "sudo_permission" => sudo_permission(validator).await,
        "password_locked" => password_locked(validator),
        "login_shell" => login_shell(validator),
        "home_directory" => home_directory(validator),
        "acl_contains" => acl_contains(validator).await,
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

fn user_exists(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let Ok(entries) = sys::read_passwd() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/passwd");
    };
    match entries.iter().find(|e| e.name == user) {
        Some(entry) => {
            // uidMin distinguishes "you created a real account" from "you reused a system uid".
            if let Some(minimum) = args::optional_integer(v, "uidMin") {
                if (entry.uid as i64) < minimum {
                    return CheckOutcome::fail(
                        &v.kind,
                        format!(
                            "The account {user} exists but has uid {}, which is in the system \
                             range rather than the regular user range.",
                            entry.uid
                        ),
                        FailureCategory::TaskPartiallyCompleted,
                    )
                    .expected(format!("uid of at least {minimum}"))
                    .observed(entry.uid.to_string());
                }
            }
            CheckOutcome::pass(&v.kind, format!("The account {user} exists."))
                .observed(format!("uid {}", entry.uid))
        }
        None => CheckOutcome::fail(
            &v.kind,
            format!("There is no account called {user}."),
            FailureCategory::TaskPartiallyCompleted,
        ),
    }
}

fn user_missing(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let Ok(entries) = sys::read_passwd() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/passwd");
    };
    if entries.iter().any(|e| e.name == user) {
        CheckOutcome::fail(
            &v.kind,
            format!("The account {user} still exists."),
            FailureCategory::TaskPartiallyCompleted,
        )
    } else {
        CheckOutcome::pass(&v.kind, format!("There is no account called {user}."))
    }
}

fn group_exists(v: &Validator) -> CheckOutcome {
    let group = arg!(args::string(v, "group"));
    let Ok(groups) = sys::read_groups() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/group");
    };
    if groups.iter().any(|g| g.name == group) {
        CheckOutcome::pass(&v.kind, format!("The group {group} exists."))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("There is no group called {group}."),
            FailureCategory::TaskPartiallyCompleted,
        )
    }
}

fn group_membership(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let group = arg!(args::string(v, "group"));

    let Ok(passwd) = sys::read_passwd() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/passwd");
    };
    let Some(entry) = passwd.iter().find(|e| e.name == user) else {
        return CheckOutcome::fail(
            &v.kind,
            format!("There is no account called {user}."),
            FailureCategory::TaskPartiallyCompleted,
        );
    };
    let Ok(groups) = sys::read_groups() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/group");
    };
    let Some(target) = groups.iter().find(|g| g.name == group) else {
        return CheckOutcome::fail(
            &v.kind,
            format!("There is no group called {group}."),
            FailureCategory::TaskPartiallyCompleted,
        );
    };

    if args::flag(v, "primary") {
        return if entry.gid == target.gid {
            CheckOutcome::pass(&v.kind, format!("{group} is the primary group of {user}."))
        } else {
            let actual = sys::group_for_gid(entry.gid).unwrap_or_else(|| entry.gid.to_string());
            CheckOutcome::fail(
                &v.kind,
                format!("{group} is not the primary group of {user}."),
                FailureCategory::WrongOwnership,
            )
            .expected(group)
            .observed(actual)
        };
    }

    let member = target.members.iter().any(|m| m == user) || entry.gid == target.gid;
    if member {
        CheckOutcome::pass(&v.kind, format!("{user} is a member of {group}."))
    } else {
        let actual = sys::groups_for_user(user).unwrap_or_default();
        CheckOutcome::fail(
            &v.kind,
            format!(
                "{user} is not in the {group} group. Note that adding a user to a group does not \
                 change the groups of sessions that are already open."
            ),
            FailureCategory::WrongOwnership,
        )
        .expected(group)
        .observed(actual.join(", "))
    }
}

/// Reads sudo's own view of the rules rather than parsing sudoers, so an `#includedir` drop-in
/// or a group-based rule is evaluated the way sudo would evaluate it.
async fn sudo_permission(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let expected_allowed = args::flag_or(v, "allowed", true);
    let command = args::optional_string(v, "command");

    let mut arguments: Vec<&str> = vec!["-l", "-U", user];
    if let Some(command) = command {
        arguments.push("--");
        arguments.push(command);
    }

    let output = match sys::run("sudo", &arguments, None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    // With an explicit command, sudo -l exits zero only when that command is permitted.
    // Without one, it lists the rules, and "not allowed to run sudo" appears in the output.
    let allowed = if command.is_some() {
        output.success()
    } else {
        output.success() && !output.stdout.contains("not allowed to run sudo")
    };

    let subject = command
        .map(|c| format!("run {c} with sudo"))
        .unwrap_or_else(|| "use sudo".to_string());

    match (allowed, expected_allowed) {
        (true, true) => CheckOutcome::pass(&v.kind, format!("{user} may {subject}.")),
        (false, false) => CheckOutcome::pass(&v.kind, format!("{user} may not {subject}.")),
        (true, false) => CheckOutcome::fail(
            &v.kind,
            format!("{user} can still {subject}, but should not be able to."),
            FailureCategory::PermissionDenied,
        ),
        (false, true) => CheckOutcome::fail(
            &v.kind,
            format!("{user} cannot {subject} yet."),
            FailureCategory::PermissionDenied,
        ),
    }
}

/// A locked password is `!` or `*` in the shadow hash field, or an empty field.
fn password_locked(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let expected_locked = args::flag_or(v, "locked", true);

    let Ok(shadow) = std::fs::read_to_string("/etc/shadow") else {
        return CheckOutcome::error(
            &v.kind,
            "could not read /etc/shadow; the agent must run as root",
        );
    };
    let Some(hash) = shadow.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == user).then(|| fields.next().unwrap_or("").to_string())
    }) else {
        return CheckOutcome::fail(
            &v.kind,
            format!("There is no shadow entry for {user}."),
            FailureCategory::TaskPartiallyCompleted,
        );
    };

    let locked = hash.is_empty() || hash.starts_with('!') || hash.starts_with('*');
    if locked == expected_locked {
        CheckOutcome::pass(
            &v.kind,
            format!(
                "The password for {user} is {}.",
                if locked { "locked" } else { "usable" }
            ),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!(
                "The password for {user} is {}.",
                if locked { "locked" } else { "not locked" }
            ),
            FailureCategory::PermissionDenied,
        )
        .expected(if expected_locked {
            "locked"
        } else {
            "not locked"
        })
        .observed(if locked { "locked" } else { "not locked" })
    }
}

fn login_shell(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let expected = arg!(args::string(v, "shell"));
    let Ok(entries) = sys::read_passwd() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/passwd");
    };
    match entries.iter().find(|e| e.name == user) {
        Some(entry) if entry.shell == expected => CheckOutcome::pass(
            &v.kind,
            format!("The login shell for {user} is {expected}."),
        ),
        Some(entry) => CheckOutcome::fail(
            &v.kind,
            format!("The login shell for {user} is not what the task asked for."),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected(expected)
        .observed(&entry.shell),
        None => CheckOutcome::fail(
            &v.kind,
            format!("There is no account called {user}."),
            FailureCategory::TaskPartiallyCompleted,
        ),
    }
}

fn home_directory(v: &Validator) -> CheckOutcome {
    let user = arg!(args::string(v, "user"));
    let expected = arg!(args::string(v, "path"));
    let Ok(entries) = sys::read_passwd() else {
        return CheckOutcome::error(&v.kind, "could not read /etc/passwd");
    };
    let Some(entry) = entries.iter().find(|e| e.name == user) else {
        return CheckOutcome::fail(
            &v.kind,
            format!("There is no account called {user}."),
            FailureCategory::TaskPartiallyCompleted,
        );
    };
    if entry.home != expected {
        return CheckOutcome::fail(
            &v.kind,
            format!("The home directory recorded for {user} is not what the task asked for."),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected(expected)
        .observed(&entry.home);
    }
    if args::flag(v, "mustExist") && !std::path::Path::new(expected).is_dir() {
        // useradd without -m is exactly this situation, and it is worth naming.
        return CheckOutcome::fail(
            &v.kind,
            format!(
                "{user} is configured with the home directory {expected}, but that directory does \
                 not exist. Creating a user does not create their home directory unless you ask \
                 for it."
            ),
            FailureCategory::WrongPath,
        );
    }
    CheckOutcome::pass(
        &v.kind,
        format!("The home directory for {user} is {expected}."),
    )
}

async fn acl_contains(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let entry = arg!(args::string(v, "entry"));
    let want_default = args::flag(v, "isDefault");

    let output = match sys::run(
        "getfacl",
        &["--omit-header", "--", &path.to_string_lossy()],
        None,
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if !output.success() {
        return CheckOutcome::fail(
            &v.kind,
            format!(
                "Could not read the ACL of {}. Does it exist?",
                path.display()
            ),
            FailureCategory::WrongPath,
        );
    }

    let lines: Vec<&str> = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();

    let found = lines.iter().any(|line| {
        let is_default = line.starts_with("default:");
        let body = line.strip_prefix("default:").unwrap_or(line);
        is_default == want_default && body == entry
    });

    if found {
        CheckOutcome::pass(
            &v.kind,
            format!("{} has the expected ACL entry.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} does not have the expected ACL entry.", path.display()),
            FailureCategory::WrongPermissions,
        )
        .expected(if want_default {
            format!("default:{entry}")
        } else {
            entry.to_string()
        })
        .observed(lines.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_exists_on_any_linux_host() {
        let outcome = user_exists(&Validator::new("user_exists").with("user", "root"));
        assert!(outcome.passed, "{}", outcome.message);
        assert_eq!(outcome.observed.as_deref(), Some("uid 0"));
    }

    #[test]
    fn root_fails_a_regular_user_uid_floor() {
        // This is how a lesson checks the learner made a real account rather than a system one.
        let outcome = user_exists(
            &Validator::new("user_exists")
                .with("user", "root")
                .with("uidMin", 1000),
        );
        assert!(!outcome.passed);
        assert!(outcome.message.contains("system"), "{}", outcome.message);
    }

    #[test]
    fn an_invented_account_is_reported_missing() {
        let outcome =
            user_exists(&Validator::new("user_exists").with("user", "definitely-not-a-user-xyzzy"));
        assert!(!outcome.passed);
        let inverse = user_missing(
            &Validator::new("user_missing").with("user", "definitely-not-a-user-xyzzy"),
        );
        assert!(inverse.passed);
    }

    #[test]
    fn root_group_membership_is_detected_through_the_primary_gid() {
        // root's primary group is root but it is usually not listed as a member, which is the
        // case a naive /etc/group scan gets wrong.
        let outcome = group_membership(
            &Validator::new("group_membership")
                .with("user", "root")
                .with("group", "root"),
        );
        assert!(outcome.passed, "{}", outcome.message);
    }

    #[test]
    fn group_membership_failure_lists_the_groups_the_user_is_actually_in() {
        let Ok(groups) = sys::read_groups() else {
            return;
        };
        // Find a group root is definitely not in, if one exists on this host.
        let Some(other) = groups
            .iter()
            .find(|g| g.name != "root" && !g.members.iter().any(|m| m == "root") && g.gid != 0)
        else {
            return;
        };
        let outcome = group_membership(
            &Validator::new("group_membership")
                .with("user", "root")
                .with("group", &*other.name),
        );
        assert!(!outcome.passed);
        assert!(outcome.observed.is_some());
        assert!(
            outcome.message.contains("already open"),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn login_shell_mismatch_reports_both_sides() {
        let outcome = login_shell(
            &Validator::new("login_shell")
                .with("user", "root")
                .with("shell", "/definitely/not/a/shell"),
        );
        assert!(!outcome.passed);
        assert_eq!(outcome.expected.as_deref(), Some("/definitely/not/a/shell"));
        assert!(outcome.observed.is_some());
    }

    #[test]
    fn home_directory_must_exist_only_when_asked() {
        let lenient = home_directory(
            &Validator::new("home_directory")
                .with("user", "root")
                .with("path", "/root"),
        );
        assert!(lenient.passed, "{}", lenient.message);

        let strict = home_directory(
            &Validator::new("home_directory")
                .with("user", "root")
                .with("path", "/root")
                .with("mustExist", true),
        );
        // /root exists on a normal host; if it does not, the message must explain why.
        if !strict.passed {
            assert!(
                strict.message.contains("does not exist"),
                "{}",
                strict.message
            );
        }
    }

    #[test]
    fn a_nonexistent_group_is_a_clear_failure_not_an_error() {
        let outcome = group_exists(
            &Validator::new("group_exists").with("group", "definitely-not-a-group-xyzzy"),
        );
        assert!(!outcome.passed);
        assert!(!outcome.errored);
    }
}
