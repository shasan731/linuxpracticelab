//! Filesystem validators.
//!
//! Everything here inspects the actual filesystem. Nothing consults shell history: a learner
//! who reaches the required state by an unexpected route passes, and a learner who types the
//! suggested command but in the wrong directory does not (spec 5.4, 9.3).

use super::args::{self, Bounds};
use super::Ctx;
use shared_types::{CheckOutcome, FailureCategory, Validator};
use std::path::Path;

use crate::sys::{self, FileKind};

pub async fn dispatch(ctx: &Ctx, validator: &Validator) -> Option<CheckOutcome> {
    let outcome = match validator.kind.as_str() {
        "file_exists" => file_exists(validator),
        "file_missing" => file_missing(validator),
        "directory_exists" => directory_exists(validator),
        "directory_missing" => directory_missing(validator),
        "symbolic_link_exists" => symbolic_link_exists(validator),
        "hard_link_exists" => hard_link_exists(validator),
        "file_type" => file_type(validator),
        "file_size" => file_size(validator),
        "file_owner" => file_owner(validator),
        "file_group" => file_group(validator),
        "file_mode" => file_mode(validator),
        "file_contains" => file_contains(validator),
        "file_matches_regex" => file_matches_regex(validator),
        "file_line_count" => file_line_count(validator),
        "file_checksum" => file_checksum(validator).await,
        "directory_contains" => directory_contains(validator),
        "directory_empty" => directory_empty(validator),
        "current_directory" => current_directory(ctx, validator),
        _ => return None,
    };
    Some(outcome)
}

fn wrong_path(validator: &Validator, message: String) -> CheckOutcome {
    CheckOutcome::fail(&validator.kind, message, FailureCategory::WrongPath)
}

fn run<T>(result: Result<T, CheckOutcome>) -> Result<T, CheckOutcome> {
    result
}

macro_rules! arg {
    ($e:expr) => {
        match run($e) {
            Ok(value) => value,
            Err(outcome) => return outcome,
        }
    };
}

fn file_exists(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    match sys::stat(&path) {
        Some(facts) if facts.kind == FileKind::Regular => {
            CheckOutcome::pass(&v.kind, format!("{} exists.", path.display()))
        }
        Some(facts) => CheckOutcome::fail(
            &v.kind,
            format!(
                "{} exists but is a {}, not a file.",
                path.display(),
                facts.kind.as_str()
            ),
            FailureCategory::WrongFileType,
        )
        .expected("regular file")
        .observed(facts.kind.as_str()),
        None => wrong_path(v, format!("{} does not exist.", path.display())),
    }
}

fn file_missing(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    if sys::lstat(&path).is_none() {
        CheckOutcome::pass(&v.kind, format!("{} is gone.", path.display()))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} still exists.", path.display()),
            FailureCategory::FileAlreadyExists,
        )
    }
}

fn directory_exists(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    match sys::stat(&path) {
        Some(facts) if facts.kind == FileKind::Directory => {
            CheckOutcome::pass(&v.kind, format!("The directory {} exists.", path.display()))
        }
        Some(facts) => CheckOutcome::fail(
            &v.kind,
            format!(
                "{} exists but is a {}, not a directory.",
                path.display(),
                facts.kind.as_str()
            ),
            FailureCategory::WrongFileType,
        )
        .expected("directory")
        .observed(facts.kind.as_str()),
        None => {
            // Naming the deepest parent that *does* exist turns "wrong path" into an
            // actionable hint, and is how a learner spots a missing mkdir -p.
            let hint = deepest_existing_parent(&path)
                .map(|parent| format!(" The closest existing directory is {}.", parent.display()))
                .unwrap_or_default();
            wrong_path(
                v,
                format!("The directory {} does not exist.{hint}", path.display()),
            )
        }
    }
}

fn directory_missing(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    match sys::lstat(&path) {
        Some(facts) if facts.kind == FileKind::Directory => CheckOutcome::fail(
            &v.kind,
            format!("The directory {} still exists.", path.display()),
            FailureCategory::FileAlreadyExists,
        ),
        _ => CheckOutcome::pass(&v.kind, format!("{} is not a directory.", path.display())),
    }
}

fn deepest_existing_parent(path: &Path) -> Option<std::path::PathBuf> {
    let mut current = path.parent()?.to_path_buf();
    loop {
        if current.is_dir() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn symbolic_link_exists(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let Some(facts) = sys::lstat(&path) else {
        return wrong_path(v, format!("{} does not exist.", path.display()));
    };
    if facts.kind != FileKind::Symlink {
        return CheckOutcome::fail(
            &v.kind,
            format!(
                "{} exists but is a {}, not a symbolic link.",
                path.display(),
                facts.kind.as_str()
            ),
            FailureCategory::WrongFileType,
        )
        .expected("symbolic link")
        .observed(facts.kind.as_str());
    }

    let Some(expected_target) = args::optional_string(v, "target") else {
        return CheckOutcome::pass(&v.kind, format!("{} is a symbolic link.", path.display()));
    };

    let actual = if args::flag(v, "resolved") {
        std::fs::canonicalize(&path)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        std::fs::read_link(&path)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    };

    match actual {
        Some(actual) if actual == expected_target => CheckOutcome::pass(
            &v.kind,
            format!("{} points at {expected_target}.", path.display()),
        ),
        Some(actual) => CheckOutcome::fail(
            &v.kind,
            format!("{} points somewhere else.", path.display()),
            FailureCategory::WrongPath,
        )
        .expected(expected_target)
        .observed(actual),
        None => CheckOutcome::fail(
            &v.kind,
            format!("{} is a broken symbolic link.", path.display()),
            FailureCategory::WrongPath,
        ),
    }
}

fn hard_link_exists(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let link_to = arg!(args::path(v, "linkTo"));

    let (Some(a), Some(b)) = (sys::lstat(&path), sys::lstat(&link_to)) else {
        return wrong_path(
            v,
            format!(
                "{} and {} must both exist for a hard link to be checked.",
                path.display(),
                link_to.display()
            ),
        );
    };

    if a.inode == b.inode {
        CheckOutcome::pass(
            &v.kind,
            format!(
                "{} and {} are the same file (inode {}).",
                path.display(),
                link_to.display(),
                a.inode
            ),
        )
    } else {
        // Distinguishing a copy from a link is the whole point of the links lesson.
        CheckOutcome::fail(
            &v.kind,
            format!(
                "{} and {} are separate files rather than hard links to one inode. A copy is \
                 not a link.",
                path.display(),
                link_to.display()
            ),
            FailureCategory::WrongFileType,
        )
        .expected("one shared inode")
        .observed(format!("inodes {} and {}", a.inode, b.inode))
    }
}

fn file_type(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let expected = arg!(args::string(v, "fileType"));
    // A symlink check must not follow the link, or every symlink would look like its target.
    let facts = if expected == "symlink" {
        sys::lstat(&path)
    } else {
        sys::stat(&path)
    };
    match facts {
        Some(facts) if facts.kind.as_str() == expected => {
            CheckOutcome::pass(&v.kind, format!("{} is a {expected}.", path.display()))
        }
        Some(facts) => CheckOutcome::fail(
            &v.kind,
            format!("{} is the wrong kind of object.", path.display()),
            FailureCategory::WrongFileType,
        )
        .expected(expected)
        .observed(facts.kind.as_str()),
        None => wrong_path(v, format!("{} does not exist.", path.display())),
    }
}

fn file_size(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let bounds = Bounds::read(v);
    match sys::stat(&path) {
        Some(facts) if bounds.satisfied_by(facts.size as i64) => CheckOutcome::pass(
            &v.kind,
            format!("{} is {} bytes.", path.display(), facts.size),
        ),
        Some(facts) => CheckOutcome::fail(
            &v.kind,
            format!("{} is not the expected size.", path.display()),
            FailureCategory::WrongFileContents,
        )
        .expected(format!("{} bytes", bounds.describe()))
        .observed(format!("{} bytes", facts.size)),
        None => wrong_path(v, format!("{} does not exist.", path.display())),
    }
}

fn file_owner(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let expected = arg!(args::string(v, "owner"));
    let Some(facts) = sys::stat(&path) else {
        return wrong_path(v, format!("{} does not exist.", path.display()));
    };
    let actual = sys::user_for_uid(facts.uid).unwrap_or_else(|| facts.uid.to_string());
    if actual == expected {
        CheckOutcome::pass(
            &v.kind,
            format!("{} is owned by {expected}.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} has the wrong owner.", path.display()),
            FailureCategory::WrongOwnership,
        )
        .expected(expected)
        .observed(actual)
    }
}

fn file_group(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let expected = arg!(args::string(v, "group"));
    let Some(facts) = sys::stat(&path) else {
        return wrong_path(v, format!("{} does not exist.", path.display()));
    };
    let actual = sys::group_for_gid(facts.gid).unwrap_or_else(|| facts.gid.to_string());
    if actual == expected {
        CheckOutcome::pass(
            &v.kind,
            format!("{} belongs to group {expected}.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} has the wrong group.", path.display()),
            FailureCategory::WrongOwnership,
        )
        .expected(expected)
        .observed(actual)
    }
}

fn file_mode(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let Some(expected) = v.params.get("mode").and_then(sys::parse_mode) else {
        return CheckOutcome::error(&v.kind, "the mode parameter is not a valid octal mode");
    };
    // Comparing through a mask lets a task say "the group must not be able to write" without
    // dictating every other bit, which is what makes several valid answers acceptable.
    let mask = v
        .params
        .get("mask")
        .and_then(sys::parse_mode)
        .unwrap_or(0o7777);

    let Some(facts) = sys::stat(&path) else {
        return wrong_path(v, format!("{} does not exist.", path.display()));
    };

    if facts.permissions & mask == expected & mask {
        CheckOutcome::pass(
            &v.kind,
            format!(
                "{} has permissions {}.",
                path.display(),
                sys::format_mode(facts.permissions)
            ),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} does not have the required permissions.", path.display()),
            FailureCategory::WrongPermissions,
        )
        .expected(if mask == 0o7777 {
            sys::format_mode(expected)
        } else {
            format!(
                "{} in the bits {}",
                sys::format_mode(expected),
                sys::format_mode(mask)
            )
        })
        .observed(sys::format_mode(facts.permissions))
    }
}

fn read_text(path: &Path) -> Result<String, CheckOutcome> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
        .map_err(|err| {
            let category = if err.kind() == std::io::ErrorKind::PermissionDenied {
                FailureCategory::PermissionDenied
            } else {
                FailureCategory::WrongPath
            };
            CheckOutcome::fail(
                "read",
                format!("{} could not be read ({err}).", path.display()),
                category,
            )
        })
}

fn file_contains(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let needle = arg!(args::string(v, "text"));
    let content = arg!(read_text(&path));
    let case_sensitive = args::flag_or(v, "caseSensitive", true);
    let trim = args::flag(v, "trim");

    let normalise = |text: &str| {
        let text = if trim {
            text.lines().map(str::trim).collect::<Vec<_>>().join("\n")
        } else {
            text.to_string()
        };
        if case_sensitive {
            text
        } else {
            text.to_lowercase()
        }
    };

    if normalise(&content).contains(&normalise(needle)) {
        CheckOutcome::pass(
            &v.kind,
            format!("{} contains the expected text.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} does not contain the expected text.", path.display()),
            FailureCategory::WrongFileContents,
        )
        .expected(needle)
    }
}

fn file_matches_regex(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let pattern = arg!(args::string(v, "pattern"));
    let content = arg!(read_text(&path));

    let regex = match regex::RegexBuilder::new(pattern)
        .case_insensitive(!args::flag_or(v, "caseSensitive", true))
        .multi_line(args::flag_or(v, "multiline", true))
        .build()
    {
        Ok(regex) => regex,
        Err(err) => {
            return CheckOutcome::error(&v.kind, format!("the pattern is not a valid regex: {err}"))
        }
    };

    if regex.is_match(&content) {
        CheckOutcome::pass(
            &v.kind,
            format!("{} matches the expected pattern.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} does not match the expected pattern.", path.display()),
            FailureCategory::WrongFileContents,
        )
        .expected(pattern)
    }
}

fn file_line_count(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let content = arg!(read_text(&path));
    let bounds = Bounds::read(v);
    // Count lines the way wc -l does: by newline terminators.
    let actual = content.lines().count() as i64;

    if bounds.satisfied_by(actual) {
        CheckOutcome::pass(&v.kind, format!("{} has {actual} lines.", path.display()))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!(
                "{} does not have the expected number of lines.",
                path.display()
            ),
            FailureCategory::WrongFileContents,
        )
        .expected(bounds.describe())
        .observed(actual.to_string())
    }
}

async fn file_checksum(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let expected = arg!(args::string(v, "sha256"));
    let output = match sys::run("sha256sum", &["--", &path.to_string_lossy()], None).await {
        Ok(output) => output,
        Err(err) => return CheckOutcome::error(&v.kind, err.to_string()),
    };
    if !output.success() {
        return wrong_path(
            v,
            format!("{} could not be hashed. Does it exist?", path.display()),
        );
    }
    let actual = output
        .stdout_trimmed()
        .split_whitespace()
        .next()
        .unwrap_or_default();
    if actual.eq_ignore_ascii_case(expected) {
        CheckOutcome::pass(
            &v.kind,
            format!("{} has the expected contents.", path.display()),
        )
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} does not have the expected contents.", path.display()),
            FailureCategory::WrongFileContents,
        )
        .expected(expected)
        .observed(actual)
    }
}

fn list_entries(path: &Path, include_hidden: bool) -> Result<Vec<String>, CheckOutcome> {
    let entries = std::fs::read_dir(path).map_err(|err| {
        let category = if err.kind() == std::io::ErrorKind::PermissionDenied {
            FailureCategory::PermissionDenied
        } else {
            FailureCategory::WrongPath
        };
        CheckOutcome::fail(
            "read_dir",
            format!("{} could not be listed ({err}).", path.display()),
            category,
        )
    })?;
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| include_hidden || !name.starts_with('.'))
        .collect();
    names.sort();
    Ok(names)
}

fn directory_contains(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let expected = arg!(args::string_list(v, "entries"));
    let include_hidden = args::flag_or(v, "includeHidden", true);
    let actual = arg!(list_entries(&path, include_hidden));

    let missing: Vec<&String> = expected.iter().filter(|e| !actual.contains(e)).collect();
    if !missing.is_empty() {
        return CheckOutcome::fail(
            &v.kind,
            format!(
                "{} is missing {}.",
                path.display(),
                missing
                    .iter()
                    .map(|m| m.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            FailureCategory::TaskPartiallyCompleted,
        )
        .expected(expected.join(", "))
        .observed(if actual.is_empty() {
            "an empty directory".to_string()
        } else {
            actual.join(", ")
        });
    }

    if args::flag(v, "exact") {
        let extra: Vec<&String> = actual.iter().filter(|a| !expected.contains(a)).collect();
        if !extra.is_empty() {
            return CheckOutcome::fail(
                &v.kind,
                format!(
                    "{} also contains {}, which should not be there.",
                    path.display(),
                    extra
                        .iter()
                        .map(|e| e.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                FailureCategory::TaskPartiallyCompleted,
            )
            .expected(format!("only {}", expected.join(", ")))
            .observed(actual.join(", "));
        }
    }

    CheckOutcome::pass(
        &v.kind,
        format!("{} contains everything the task asked for.", path.display()),
    )
}

fn directory_empty(v: &Validator) -> CheckOutcome {
    let path = arg!(args::path(v, "path"));
    let include_hidden = args::flag_or(v, "includeHidden", true);
    let actual = arg!(list_entries(&path, include_hidden));
    if actual.is_empty() {
        CheckOutcome::pass(&v.kind, format!("{} is empty.", path.display()))
    } else {
        CheckOutcome::fail(
            &v.kind,
            format!("{} is not empty yet.", path.display()),
            FailureCategory::TaskPartiallyCompleted,
        )
        .observed(actual.join(", "))
    }
}

fn current_directory(ctx: &Ctx, v: &Validator) -> CheckOutcome {
    let expected = arg!(args::path(v, "path"));
    let Some(shell) = sys::find_login_shell(&ctx.subject_user) else {
        return CheckOutcome::error(
            &v.kind,
            format!(
                "could not find an interactive shell for {} to inspect",
                ctx.subject_user
            ),
        );
    };
    let Some(actual) = sys::process_cwd(shell.pid) else {
        return CheckOutcome::error(
            &v.kind,
            format!(
                "could not read the working directory of process {}",
                shell.pid
            ),
        );
    };

    // Resolving both sides keeps /home/student and a symlinked path from disagreeing.
    let (expected_cmp, actual_cmp) = if args::flag_or(v, "resolveSymlinks", true) {
        (
            std::fs::canonicalize(&expected).unwrap_or_else(|_| expected.clone()),
            std::fs::canonicalize(&actual).unwrap_or_else(|_| actual.clone()),
        )
    } else {
        (expected.clone(), actual.clone())
    };

    if expected_cmp == actual_cmp {
        CheckOutcome::pass(&v.kind, format!("Your shell is in {}.", actual.display()))
    } else {
        CheckOutcome::fail(
            &v.kind,
            "Your shell is not in the directory the task asked for. Run pwd to see where you are."
                .to_string(),
            FailureCategory::WrongWorkingDirectory,
        )
        .expected(expected.display().to_string())
        .observed(actual.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validator(kind: &str) -> Validator {
        Validator::new(kind)
    }

    #[test]
    fn file_exists_passes_for_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("january.txt");
        std::fs::write(&path, b"data").unwrap();

        let outcome = file_exists(&validator("file_exists").with("path", path.to_str().unwrap()));
        assert!(outcome.passed, "{}", outcome.message);
    }

    #[test]
    fn file_exists_distinguishes_a_directory_from_a_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let outcome =
            file_exists(&validator("file_exists").with("path", dir.path().to_str().unwrap()));
        assert!(!outcome.passed);
        assert_eq!(
            outcome.failure_category,
            Some(FailureCategory::WrongFileType)
        );
        assert_eq!(outcome.observed.as_deref(), Some("directory"));

        let absent = file_exists(
            &validator("file_exists").with("path", dir.path().join("nope").to_str().unwrap()),
        );
        assert_eq!(absent.failure_category, Some(FailureCategory::WrongPath));
    }

    #[test]
    fn directory_exists_names_the_closest_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("projects/api/logs");
        let outcome =
            directory_exists(&validator("directory_exists").with("path", target.to_str().unwrap()));
        assert!(!outcome.passed);
        // This is the feedback that points a learner at mkdir -p.
        assert!(
            outcome.message.contains("closest existing directory"),
            "{}",
            outcome.message
        );
        assert!(outcome.message.contains(dir.path().to_str().unwrap()));
    }

    #[test]
    fn file_mode_compares_through_a_mask() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.env");
        std::fs::write(&path, b"x").unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o640))
            .unwrap();

        let exact = file_mode(
            &validator("file_mode")
                .with("path", path.to_str().unwrap())
                .with("mode", "0640"),
        );
        assert!(exact.passed, "{}", exact.message);

        // "not world readable" expressed as a mask: only the other-bits are compared.
        let masked = file_mode(
            &validator("file_mode")
                .with("path", path.to_str().unwrap())
                .with("mode", "0000")
                .with("mask", "0007"),
        );
        assert!(masked.passed, "{}", masked.message);

        let wrong = file_mode(
            &validator("file_mode")
                .with("path", path.to_str().unwrap())
                .with("mode", "0644"),
        );
        assert!(!wrong.passed);
        assert_eq!(wrong.expected.as_deref(), Some("0644"));
        assert_eq!(wrong.observed.as_deref(), Some("0640"));
    }

    #[test]
    fn file_contains_can_ignore_case_and_indentation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nginx.conf");
        std::fs::write(&path, b"    Listen 8080;\n").unwrap();

        let strict = file_contains(
            &validator("file_contains")
                .with("path", path.to_str().unwrap())
                .with("text", "listen 8080;"),
        );
        assert!(!strict.passed, "case sensitive by default");

        let relaxed = file_contains(
            &validator("file_contains")
                .with("path", path.to_str().unwrap())
                .with("text", "listen 8080;")
                .with("caseSensitive", false)
                .with("trim", true),
        );
        assert!(relaxed.passed, "{}", relaxed.message);
    }

    #[test]
    fn a_bad_regex_is_an_authoring_error_not_a_learner_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"x").unwrap();
        let outcome = file_matches_regex(
            &validator("file_matches_regex")
                .with("path", path.to_str().unwrap())
                .with("pattern", "([unclosed"),
        );
        assert!(outcome.errored);
        assert!(!outcome.passed);
    }

    #[test]
    fn line_counting_matches_wc_l() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("names.txt");
        std::fs::write(&path, b"a\nb\nc\n").unwrap();
        let outcome = file_line_count(
            &validator("file_line_count")
                .with("path", path.to_str().unwrap())
                .with("equals", 3),
        );
        assert!(outcome.passed, "{}", outcome.message);
    }

    #[test]
    fn hard_link_detection_rejects_a_copy() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original");
        let copy = dir.path().join("copy");
        let link = dir.path().join("link");
        std::fs::write(&original, b"data").unwrap();
        std::fs::copy(&original, &copy).unwrap();
        std::fs::hard_link(&original, &link).unwrap();

        let linked = hard_link_exists(
            &validator("hard_link_exists")
                .with("path", link.to_str().unwrap())
                .with("linkTo", original.to_str().unwrap()),
        );
        assert!(linked.passed, "{}", linked.message);

        let copied = hard_link_exists(
            &validator("hard_link_exists")
                .with("path", copy.to_str().unwrap())
                .with("linkTo", original.to_str().unwrap()),
        );
        assert!(!copied.passed);
        assert!(
            copied.message.contains("A copy is not a link"),
            "{}",
            copied.message
        );
    }

    #[test]
    fn symlink_target_is_compared_without_following_it() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("original");
        std::fs::write(&target, b"x").unwrap();
        let link = dir.path().join("shortcut");
        std::os::unix::fs::symlink("original", &link).unwrap();

        let matching = symbolic_link_exists(
            &validator("symbolic_link_exists")
                .with("path", link.to_str().unwrap())
                .with("target", "original"),
        );
        assert!(matching.passed, "{}", matching.message);

        let mismatched = symbolic_link_exists(
            &validator("symbolic_link_exists")
                .with("path", link.to_str().unwrap())
                .with("target", "somewhere-else"),
        );
        assert!(!mismatched.passed);
        assert_eq!(mismatched.observed.as_deref(), Some("original"));
    }

    #[test]
    fn file_type_symlink_does_not_follow_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("dir");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let as_symlink = file_type(
            &validator("file_type")
                .with("path", link.to_str().unwrap())
                .with("fileType", "symlink"),
        );
        assert!(as_symlink.passed, "{}", as_symlink.message);

        let as_directory = file_type(
            &validator("file_type")
                .with("path", link.to_str().unwrap())
                .with("fileType", "directory"),
        );
        assert!(as_directory.passed, "following the link is correct here");
    }

    #[test]
    fn directory_contains_reports_exactly_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("daily"), b"").unwrap();
        let outcome = directory_contains(
            &validator("directory_contains")
                .with("path", dir.path().to_str().unwrap())
                .with("entries", serde_json::json!(["daily", "monthly"])),
        );
        assert!(!outcome.passed);
        assert!(
            outcome.message.contains("missing monthly"),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn directory_contains_exact_rejects_extra_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("daily"), b"").unwrap();
        std::fs::write(dir.path().join("scratch.tmp"), b"").unwrap();
        let outcome = directory_contains(
            &validator("directory_contains")
                .with("path", dir.path().to_str().unwrap())
                .with("entries", serde_json::json!(["daily"]))
                .with("exact", true),
        );
        assert!(!outcome.passed);
        assert!(
            outcome.message.contains("scratch.tmp"),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn directory_empty_ignores_hidden_files_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".keep"), b"").unwrap();

        let strict = directory_empty(
            &validator("directory_empty").with("path", dir.path().to_str().unwrap()),
        );
        assert!(!strict.passed, "hidden files count by default");

        let relaxed = directory_empty(
            &validator("directory_empty")
                .with("path", dir.path().to_str().unwrap())
                .with("includeHidden", false),
        );
        assert!(relaxed.passed, "{}", relaxed.message);
    }

    #[test]
    fn file_missing_passes_for_a_dangling_symlink_target_but_not_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();

        // The link itself exists even though its target does not.
        let link_check =
            file_missing(&validator("file_missing").with("path", link.to_str().unwrap()));
        assert!(!link_check.passed);

        let target_check = file_missing(
            &validator("file_missing").with("path", dir.path().join("gone").to_str().unwrap()),
        );
        assert!(target_check.passed);
    }
}
