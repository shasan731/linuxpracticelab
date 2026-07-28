//! Package validators, backed by dpkg and apt's own state rather than by parsing apt output.

use super::args;
use super::Ctx;
use crate::sys;
use shared_types::{CheckOutcome, FailureCategory, Validator};

pub async fn dispatch(_ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "package_installed" => package_installed(validator).await,
        "package_removed" => package_removed(validator).await,
        "repository_configured" => repository_configured(validator),
        "package_version" => package_version(validator).await,
        "apt_cache_updated" => apt_cache_updated(validator),
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

/// dpkg's status and version for a package, if it knows about it at all.
async fn dpkg_status(package: &str) -> Option<(String, String)> {
    let output = sys::run(
        "dpkg-query",
        &["-W", "-f=${db:Status-Status} ${Version}", "--", package],
        None,
    )
    .await
    .ok()?;
    if !output.success() {
        return None;
    }
    let text = output.stdout_trimmed();
    let mut parts = text.splitn(2, ' ');
    let status = parts.next()?.to_string();
    let version = parts.next().unwrap_or("").trim().to_string();
    Some((status, version))
}

async fn package_installed(v: &Validator) -> CheckOutcome {
    let package = arg!(args::string(v, "package"));
    match dpkg_status(package).await {
        Some((status, version)) if status == "installed" => {
            if let Some(expected) = args::optional_string(v, "version") {
                if version != expected {
                    return CheckOutcome::fail(
                        &v.kind,
                        format!("{package} is installed, but not at the expected version."),
                        FailureCategory::TaskPartiallyCompleted,
                    )
                    .expected(expected)
                    .observed(version);
                }
            }
            CheckOutcome::pass(&v.kind, format!("{package} is installed.")).observed(version)
        }
        Some((status, _)) => CheckOutcome::fail(
            &v.kind,
            format!(
                "{package} is known to the system but its state is '{status}' rather than \
                 installed."
            ),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected("installed")
        .observed(status),
        None => CheckOutcome::fail(
            &v.kind,
            format!("{package} is not installed."),
            FailureCategory::TaskPartiallyCompleted,
        ),
    }
}

async fn package_removed(v: &Validator) -> CheckOutcome {
    let package = arg!(args::string(v, "package"));
    let purged = args::flag(v, "purged");
    match dpkg_status(package).await {
        None => CheckOutcome::pass(&v.kind, format!("{package} is not present.")),
        Some((status, _)) if status == "installed" => CheckOutcome::fail(
            &v.kind,
            format!("{package} is still installed."),
            FailureCategory::TaskPartiallyCompleted,
        )
        .observed(status),
        Some((status, _)) if purged && status == "config-files" => CheckOutcome::fail(
            &v.kind,
            format!(
                "{package} was removed, but its configuration files are still there. Removing a \
                 package and purging it are different operations."
            ),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected("no configuration files left")
        .observed(status),
        Some((status, _)) => {
            CheckOutcome::pass(&v.kind, format!("{package} is removed.")).observed(status)
        }
    }
}

fn repository_configured(v: &Validator) -> CheckOutcome {
    let uri = arg!(args::string(v, "uri"));

    let mut sources: Vec<(String, String)> = Vec::new();
    let mut push = |path: std::path::PathBuf| {
        if let Ok(content) = std::fs::read_to_string(&path) {
            sources.push((path.display().to_string(), content));
        }
    };

    if let Some(file) = args::optional_string(v, "file") {
        push(std::path::PathBuf::from(file));
    } else {
        push(std::path::PathBuf::from("/etc/apt/sources.list"));
        // Both the classic .list format and the deb822 .sources format are in use on Trixie.
        for directory in ["/etc/apt/sources.list.d"] {
            if let Ok(entries) = std::fs::read_dir(directory) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let is_source = path
                        .extension()
                        .map(|e| e == "list" || e == "sources")
                        .unwrap_or(false);
                    if is_source {
                        push(path);
                    }
                }
            }
        }
    }

    if sources.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            "No apt source files could be read.".to_string(),
            FailureCategory::TaskPartiallyCompleted,
        );
    }

    let found = sources.iter().find(|(_, content)| {
        content
            .lines()
            .map(str::trim)
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .any(|line| line.contains(uri))
    });

    match found {
        Some((path, _)) => {
            CheckOutcome::pass(&v.kind, format!("{uri} is configured as a package source."))
                .observed(path)
        }
        None => CheckOutcome::fail(
            &v.kind,
            format!("{uri} is not configured as a package source."),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected(uri)
        .observed(
            sources
                .iter()
                .map(|(path, _)| path.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }
}

async fn package_version(v: &Validator) -> CheckOutcome {
    let package = arg!(args::string(v, "package"));
    let expected = arg!(args::string(v, "version"));
    let comparison = args::optional_string(v, "comparison").unwrap_or("eq");

    let Some((status, actual)) = dpkg_status(package).await else {
        return CheckOutcome::fail(
            &v.kind,
            format!("{package} is not installed, so its version cannot be compared."),
            FailureCategory::TaskPartiallyCompleted,
        );
    };
    if status != "installed" {
        return CheckOutcome::fail(
            &v.kind,
            format!("{package} is not installed (state '{status}')."),
            FailureCategory::TaskPartiallyCompleted,
        );
    }

    // dpkg knows Debian version ordering, which is subtle enough that reimplementing it
    // would be a source of wrong answers.
    let output = match sys::run(
        "dpkg",
        &["--compare-versions", &actual, comparison, expected],
        None,
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };

    if output.success() {
        CheckOutcome::pass(
            &v.kind,
            format!("{package} {actual} satisfies {comparison} {expected}."),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("The installed version of {package} does not satisfy the requirement."),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected(format!("{comparison} {expected}"))
        .observed(actual)
    }
}

/// Freshness of the package lists, read from the mtime of apt's list directory.
fn apt_cache_updated(v: &Validator) -> CheckOutcome {
    let max_age = args::optional_integer(v, "maxAgeSeconds")
        .unwrap_or(3600)
        .max(1);

    // apt touches this stamp file on a successful update; it is the same signal
    // `apt-config`-aware tooling uses.
    let candidates = [
        "/var/lib/apt/periodic/update-success-stamp",
        "/var/lib/apt/lists",
    ];
    let newest = candidates
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok()?.modified().ok())
        .max();

    let Some(modified) = newest else {
        return CheckOutcome::fail(
            &v.kind,
            "The package lists have never been downloaded. Run apt update first.".to_string(),
            FailureCategory::TaskPartiallyCompleted,
        );
    };

    let age = modified
        .elapsed()
        .map(|d| d.as_secs() as i64)
        // A clock skew that puts the mtime in the future is treated as fresh, not as an error.
        .unwrap_or(0);

    if age <= max_age {
        CheckOutcome::pass(
            &v.kind,
            "The package lists have been refreshed recently.".to_string(),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            "The package lists are stale. Run apt update so apt knows what is available."
                .to_string(),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected(format!("refreshed within {max_age}s"))
        .observed(format!("{age}s old"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_source_uri_is_reported_with_the_files_that_were_searched() {
        let outcome = repository_configured(
            &Validator::new("repository_configured")
                .with("uri", "file:/opt/linuxlab/repository")
                .with("file", "/definitely/not/a/sources.list"),
        );
        assert!(!outcome.passed);
        assert!(!outcome.errored);
    }

    #[test]
    fn commented_out_sources_do_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linuxlab.list");
        std::fs::write(
            &path,
            b"# deb [trusted=yes] file:/opt/linuxlab/repository stable main\n",
        )
        .unwrap();

        let outcome = repository_configured(
            &Validator::new("repository_configured")
                .with("uri", "file:/opt/linuxlab/repository")
                .with("file", path.to_str().unwrap()),
        );
        assert!(
            !outcome.passed,
            "a commented line is not a configured source"
        );
    }

    #[test]
    fn an_active_source_line_counts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("linuxlab.list");
        std::fs::write(
            &path,
            b"deb [trusted=yes] file:/opt/linuxlab/repository stable main\n",
        )
        .unwrap();

        let outcome = repository_configured(
            &Validator::new("repository_configured")
                .with("uri", "file:/opt/linuxlab/repository")
                .with("file", path.to_str().unwrap()),
        );
        assert!(outcome.passed, "{}", outcome.message);
    }

    #[test]
    fn apt_freshness_failure_explains_the_fix() {
        let outcome =
            apt_cache_updated(&Validator::new("apt_cache_updated").with("maxAgeSeconds", 1));
        // On a host with no apt at all this fails; either way it must not error.
        assert!(!outcome.errored);
        if !outcome.passed {
            assert!(
                outcome.message.contains("apt update"),
                "{}",
                outcome.message
            );
        }
    }
}
