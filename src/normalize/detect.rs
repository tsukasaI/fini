use std::sync::LazyLock;

use regex::Regex;

use super::{Problem, ProblemKind};

const DEBUG_PATTERNS: &[&str] = &[
    "console.log(",
    "console.debug(",
    "console.warn(",
    "console.info(",
    "console.trace(",
    "console.table(",
    "console.dir(",
    "print(",
    "println!(",
    "dbg!(",
    "debugger",
];

const STRICT_DEBUG_EXTRA: &[&str] = &["console.error(", "eprintln!("];

struct SecretPattern {
    regex: Regex,
    hint: &'static str,
}

static SECRET_PATTERNS: LazyLock<Vec<SecretPattern>> = LazyLock::new(|| {
    vec![
        SecretPattern {
            regex: Regex::new(r"-----BEGIN\s+(RSA\s+)?PRIVATE\s+KEY-----").unwrap(),
            hint: "private key",
        },
        SecretPattern {
            regex: Regex::new(
                r#"(?i)(aws[_-]?)?access[_-]?key[_-]?id\s*[=:]\s*["']?AKIA[A-Z0-9]{16}["']?"#,
            )
            .unwrap(),
            hint: "AWS access key",
        },
        SecretPattern {
            regex: Regex::new(
                r#"(?i)(aws[_-]?)?secret[_-]?access[_-]?key\s*[=:]\s*["'][a-zA-Z0-9/+]{20,}["']"#,
            )
            .unwrap(),
            hint: "AWS secret key",
        },
        SecretPattern {
            regex: Regex::new(
                r#"(?i)(password|passwd|secret[_-]?key|api[_-]?key|auth[_-]?token|access[_-]?token)\s*[=:]\s*["'][a-zA-Z0-9_\-/+@#$%^&*!~.]{8,}["']"#,
            )
            .unwrap(),
            hint: "hardcoded secret",
        },
        SecretPattern {
            regex: Regex::new(r"(?i)bearer\s+[a-zA-Z0-9_\-\.]{20,}").unwrap(),
            hint: "bearer token",
        },
        SecretPattern {
            regex: Regex::new(r"ghp_[a-zA-Z0-9]{36,}").unwrap(),
            hint: "GitHub token",
        },
        SecretPattern {
            regex: Regex::new(r"xox[bpa]-[a-zA-Z0-9\-]{10,}").unwrap(),
            hint: "Slack token",
        },
        SecretPattern {
            regex: Regex::new(r"sk_(live|test)_[a-zA-Z0-9]{20,}").unwrap(),
            hint: "Stripe API key",
        },
    ]
});

const SECRET_SKIP_PATTERNS: &[&str] = &[
    "process.env",
    "os.environ",
    "std::env",
    "getenv",
    "ENV[",
    "<your-",
    "${",
    "{{",
];

/// Case-insensitive ASCII marker search without allocating an uppercase copy.
fn is_valid_marker(line: &str, marker: &str) -> bool {
    let bytes = line.as_bytes();
    let mlen = marker.len();
    for i in 0..bytes.len().saturating_sub(mlen - 1) {
        if bytes[i..i + mlen].eq_ignore_ascii_case(marker.as_bytes()) {
            let after = bytes.get(i + mlen).copied();
            return matches!(
                after,
                Some(b':') | Some(b' ') | Some(b'\t') | Some(b'(') | None
            );
        }
    }
    false
}

fn detect_comment_markers(content: &str, marker: &str, kind: ProblemKind) -> Vec<Problem> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            if is_valid_marker(line, marker) {
                Some(Problem {
                    line: line_idx + 1,
                    kind: kind.clone(),
                })
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn detect_todo_comments(content: &str) -> Vec<Problem> {
    detect_comment_markers(content, "TODO", ProblemKind::TodoComment)
}

pub(super) fn detect_fixme_comments(content: &str) -> Vec<Problem> {
    detect_comment_markers(content, "FIXME", ProblemKind::FixmeComment)
}

pub(super) fn detect_debug_code(content: &str, strict_mode: bool) -> Vec<Problem> {
    let extra: &[&str] = if strict_mode { STRICT_DEBUG_EXTRA } else { &[] };

    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            DEBUG_PATTERNS
                .iter()
                .chain(extra.iter())
                .find(|p| line.contains(*p))
                .map(|pattern| Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::DebugCode {
                        pattern: pattern.trim_end_matches('('),
                    },
                })
        })
        .collect()
}

pub(super) fn detect_secret_patterns(content: &str) -> Vec<Problem> {
    let patterns = &*SECRET_PATTERNS;

    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            if SECRET_SKIP_PATTERNS.iter().any(|p| line.contains(p)) {
                return None;
            }

            patterns
                .iter()
                .find(|p| p.regex.is_match(line))
                .map(|pattern| Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::SecretPattern { hint: pattern.hint },
                })
        })
        .collect()
}

/// Replace every line matching a secret pattern with a hint-only placeholder.
///
/// The checker deliberately reports secrets hint-only (the matched value never
/// enters a `Problem`); diff output must honor the same contract, including
/// unchanged context lines, so raw values never reach CI logs (issue #44).
pub(super) fn mask_secret_lines(content: &str) -> String {
    let patterns = &*SECRET_PATTERNS;
    let mut out = String::with_capacity(content.len());

    for line in content.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        let hint = if SECRET_SKIP_PATTERNS.iter().any(|p| text.contains(p)) {
            None
        } else {
            patterns
                .iter()
                .find(|p| p.regex.is_match(text))
                .map(|p| p.hint)
        };

        match hint {
            Some(hint) => {
                out.push_str("[line masked: potential ");
                out.push_str(hint);
                out.push(']');
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => out.push_str(line),
        }
    }

    out
}

pub(super) fn check_line_length(content: &str, max_length: usize) -> Vec<Problem> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            // byte length >= char count in UTF-8, so skip expensive chars().count() for short lines
            if line.len() <= max_length {
                return None;
            }
            let length = line.chars().count();
            (length > max_length).then_some(Problem {
                line: line_idx + 1,
                kind: ProblemKind::LongLine {
                    length,
                    limit: max_length,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_todo_basic() {
        let problems = detect_todo_comments("// TODO: fix this\n");
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, 1);
    }

    #[test]
    fn test_todo_case_insensitive() {
        assert_eq!(detect_todo_comments("// todo: fix\n").len(), 1);
        assert_eq!(detect_todo_comments("// Todo fix\n").len(), 1);
    }

    #[test]
    fn test_todo_requires_word_boundary() {
        assert!(detect_todo_comments("use todoist;\n").is_empty());
    }

    #[test]
    fn test_fixme_detected() {
        let problems = detect_fixme_comments("# FIXME: broken\n");
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn test_debug_console_log() {
        let problems = detect_debug_code("console.log('test');\n", false);
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::DebugCode { pattern } if *pattern == "console.log"
        ));
    }

    #[test]
    fn test_debug_dbg_macro() {
        let problems = detect_debug_code("dbg!(value);\n", false);
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn test_debug_strict_includes_console_error() {
        assert!(detect_debug_code("console.error('fail');\n", false).is_empty());
        assert_eq!(detect_debug_code("console.error('fail');\n", true).len(), 1);
    }

    #[test]
    fn test_debug_strict_includes_eprintln() {
        // Note: "eprintln!(" contains "println!(" as substring, so non-strict also matches
        // Strict mode adds the explicit "eprintln!(" pattern
        assert_eq!(detect_debug_code("eprintln!(\"fail\");\n", false).len(), 1);
        assert_eq!(detect_debug_code("eprintln!(\"fail\");\n", true).len(), 1);
    }

    #[test]
    fn test_secret_bearer_token() {
        let problems =
            detect_secret_patterns("Authorization: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9\n");
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::SecretPattern { hint } if *hint == "bearer token"
        ));
    }

    #[test]
    fn test_secret_github_token() {
        let problems =
            detect_secret_patterns("token = \"ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmn\"\n");
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn test_secret_skip_env_var_reference() {
        assert!(detect_secret_patterns("password = process.env.PASSWORD\n").is_empty());
        assert!(detect_secret_patterns("key = os.environ['API_KEY']\n").is_empty());
        assert!(detect_secret_patterns("key = std::env::var(\"KEY\")\n").is_empty());
    }

    #[test]
    fn test_secret_skip_placeholder() {
        assert!(detect_secret_patterns("api_key = \"<your-api-key>\"\n").is_empty());
        assert!(detect_secret_patterns("token = \"${API_TOKEN}\"\n").is_empty());
    }

    #[test]
    fn test_line_length_under_limit() {
        assert!(check_line_length("short\n", 80).is_empty());
    }

    #[test]
    fn test_line_length_at_limit() {
        let line = format!("{}\n", "a".repeat(80));
        assert!(check_line_length(&line, 80).is_empty());
    }

    #[test]
    fn test_line_length_over_limit() {
        let line = format!("{}\n", "a".repeat(81));
        let problems = check_line_length(&line, 80);
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::LongLine {
                length: 81,
                limit: 80
            }
        ));
    }

    #[test]
    fn test_line_length_multibyte_shortcut() {
        // 6 multibyte chars = 6 char count but 18 byte length
        // byte length > limit but char count <= limit should pass
        let line = "ああああああ\n"; // 6 chars, 18 bytes
        assert!(check_line_length(line, 6).is_empty());
        assert_eq!(check_line_length(line, 5).len(), 1);
    }

    #[test]
    fn test_multiple_debug_on_same_line_reports_first() {
        let problems = detect_debug_code("console.log(dbg!(x));\n", false);
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn test_mask_secret_lines_masks_matching_line() {
        let content = "password = \"supersecret123\"\nclean line\n";
        let masked = mask_secret_lines(content);
        assert!(!masked.contains("supersecret123"));
        assert!(masked.contains("[line masked: potential hardcoded secret]"));
        assert!(masked.contains("clean line\n"));
    }

    #[test]
    fn test_mask_secret_lines_keeps_skip_patterns() {
        let content = "password = process.env.PASSWORD\n";
        assert_eq!(mask_secret_lines(content), content);
    }

    #[test]
    fn test_mask_secret_lines_preserves_missing_eof_newline() {
        let content = "token = \"ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmn\"";
        let masked = mask_secret_lines(content);
        assert!(!masked.ends_with('\n'));
        assert!(!masked.contains("ghp_"));
    }

    #[test]
    fn test_is_valid_marker_boundaries() {
        assert!(is_valid_marker("// TODO: fix", "TODO"));
        assert!(is_valid_marker("// TODO fix", "TODO"));
        assert!(is_valid_marker("// TODO\ttab", "TODO"));
        assert!(is_valid_marker("// TODO(me)", "TODO"));
        assert!(is_valid_marker("TODO", "TODO"));
        assert!(!is_valid_marker("TODOLIST", "TODO"));
    }
}
