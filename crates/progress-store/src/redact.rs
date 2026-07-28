//! Transcript redaction (spec 17).
//!
//! A learner who exports a session transcript may well paste it into a bug report. Anything
//! that looks like a credential is masked first. The list is deliberately conservative and
//! matches on the *key*, not on entropy heuristics, so it never mangles ordinary output.

/// Keys whose value is masked wherever they appear.
const SENSITIVE_KEYS: &[&str] = &[
    "password=",
    "passwd=",
    "token=",
    "secret=",
    "api_key=",
    "apikey=",
    "authorization:",
    "auth_token=",
    "private_key=",
];

pub const MASK: &str = "***redacted***";

/// Masks credential-looking values in a transcript, line by line.
pub fn redact_transcript(transcript: &str) -> String {
    transcript
        .lines()
        .map(redact_line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Single forward pass over the line.
///
/// Two details matter. Keys are matched with an ASCII case-insensitive byte comparison rather
/// than by lowercasing a copy, because lowercasing can change a string's byte length and
/// desynchronise the two index spaces. And the scan resumes *after* each masked value, so an
/// inserted mask is never re-examined and re-masked.
fn redact_line(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut result = String::with_capacity(line.len());
    let mut i = 0usize;

    while i < bytes.len() {
        let matched = SENSITIVE_KEYS.iter().find(|key| {
            let key = key.as_bytes();
            i + key.len() <= bytes.len() && bytes[i..i + key.len()].eq_ignore_ascii_case(key)
        });

        if let Some(key) = matched {
            let raw_value_start = i + key.len();
            // Assignment values end at whitespace. An Authorization header is different:
            // its scheme and credential are separated by whitespace ("Bearer token"), so
            // mask the whole header value up to its quote or shell separator.
            let is_header = key.ends_with(':');
            let leading_space = if is_header {
                line[raw_value_start..]
                    .chars()
                    .take_while(|character| character.is_whitespace())
                    .map(char::len_utf8)
                    .sum()
            } else {
                0
            };
            let value_start = raw_value_start + leading_space;
            let value_end = line[value_start..]
                .find(|character: char| {
                    matches!(character, '"' | '\'' | ';' | '&' | '|')
                        || (!is_header && character.is_whitespace())
                })
                .map(|offset| value_start + offset)
                .unwrap_or(bytes.len());

            result.push_str(&line[i..value_start]);
            if value_end > value_start {
                result.push_str(MASK);
            }
            i = value_end;
        } else {
            let char_len = line[i..].chars().next().map_or(1, |c| c.len_utf8());
            result.push_str(&line[i..i + char_len]);
            i += char_len;
        }
    }
    result
}

/// The lab's own documented practice password is not a secret, and masking it would make
/// sudo lessons impossible to follow in an exported transcript.
pub fn is_lab_password(value: &str) -> bool {
    value == "linuxlab"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_password_assignments() {
        let out = redact_transcript("mysql -u root --password=hunter2 -e 'select 1'");
        assert!(!out.contains("hunter2"));
        assert!(out.contains(MASK));
        // The rest of the command survives so the transcript is still readable.
        assert!(out.contains("mysql -u root"));
        assert!(out.contains("select 1"));
    }

    #[test]
    fn masks_tokens_secrets_and_headers() {
        for line in [
            "export API_TOKEN=abc; curl -H 'authorization: Bearer xyz123' http://web1.lab",
            "echo secret=s3cr3t >> .env",
            "curl -d api_key=012345 http://web1.lab",
        ] {
            let out = redact_transcript(line);
            for leak in ["abc", "xyz123", "s3cr3t", "012345"] {
                assert!(!out.contains(leak), "leaked {leak} in {out}");
            }
        }
    }

    #[test]
    fn is_case_insensitive_about_the_key() {
        let out = redact_transcript("PASSWORD=Hunter2");
        assert!(!out.contains("Hunter2"), "{out}");
    }

    #[test]
    fn masks_several_values_on_one_line() {
        let out = redact_transcript("app --password=a --token=b");
        assert!(!out.contains("=a "));
        assert!(!out.contains("=b"));
        assert_eq!(out.matches(MASK).count(), 2, "{out}");
    }

    #[test]
    fn leaves_ordinary_output_untouched() {
        let transcript = "total 8\ndrwxr-xr-x 2 student student 4096 Jan  1 10:00 reports\nls: cannot access 'nope': No such file or directory";
        assert_eq!(redact_transcript(transcript), transcript);
    }

    #[test]
    fn an_empty_value_is_not_replaced_with_a_mask() {
        // `password=` with nothing after it has nothing to hide, and inventing a mask there
        // would be misleading.
        let out = redact_transcript("prompt: password=");
        assert_eq!(out, "prompt: password=");
    }

    #[test]
    fn the_mask_itself_is_never_re_masked() {
        // A single pass over "password=secret" must produce exactly one mask, and running the
        // redactor over its own output must be a no-op.
        let once = redact_transcript("password=secret");
        assert_eq!(once, format!("password={MASK}"));
        assert_eq!(redact_transcript(&once), once);
    }

    #[test]
    fn non_ascii_text_does_not_shift_the_masking_window() {
        let out = redact_transcript("çalışan token=gizli üstü");
        assert!(!out.contains("gizli"), "{out}");
        assert!(out.contains("çalışan"), "{out}");
        assert!(out.contains("üstü"), "{out}");
    }

    #[test]
    fn line_structure_is_preserved() {
        let out = redact_transcript("a\nb\nc");
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn the_documented_lab_password_is_recognised() {
        assert!(is_lab_password("linuxlab"));
        assert!(!is_lab_password("hunter2"));
    }
}
