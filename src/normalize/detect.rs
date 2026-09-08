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

/// Detects TODO and FIXME markers in a single pass over `content`'s lines
/// (each line is checked for both markers instead of scanning the file twice).
/// Returns the two problem kinds in separate vecs, each in line order, so
/// callers that gate TODO/FIXME detection independently can extend their
/// combined problem list with either or both without reordering results.
pub(super) fn detect_todo_and_fixme_comments(content: &str) -> (Vec<Problem>, Vec<Problem>) {
    let mut todos = Vec::new();
    let mut fixmes = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        if is_valid_marker(line, "TODO") {
            todos.push(Problem {
                line: line_idx + 1,
                kind: ProblemKind::TodoComment,
            });
        }
        if is_valid_marker(line, "FIXME") {
            fixmes.push(Problem {
                line: line_idx + 1,
                kind: ProblemKind::FixmeComment,
            });
        }
    }

    (todos, fixmes)
}

/// Substring search requiring a left word boundary at the match start (the preceding
/// byte, if any, must not be ASCII alphanumeric or `_`). Unlike `is_valid_marker`,
/// which checks the boundary *after* a marker, call-like patterns such as
/// `println!(` need the boundary checked *before* them, so a match inside
/// `eprintln!(` (offset 1) doesn't count.
fn contains_word_boundary(line: &str, pattern: &str) -> bool {
    let bytes = line.as_bytes();
    let plen = pattern.len();
    if plen == 0 {
        return false;
    }
    for i in 0..bytes.len().saturating_sub(plen - 1) {
        if bytes[i..i + plen] == *pattern.as_bytes() {
            let before = if i == 0 { None } else { Some(bytes[i - 1]) };
            if !matches!(before, Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
                return true;
            }
        }
    }
    false
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
                .find(|p| contains_word_boundary(line, p))
                .map(|pattern| Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::DebugCode {
                        pattern: pattern.trim_end_matches('('),
                    },
                })
        })
        .collect()
}

/// A skip pattern only excuses a match when it occurs within the matched text
/// itself — e.g. `token = "process.env.API_TOKEN"`, a placeholder — not when
/// it sits outside the match, such as an unrelated trailing comment
/// (`password = "hunter2hunter2"  # ${`). Checking the whole line let a skip
/// marker anywhere on the line defeat detection entirely (issue #77).
fn skip_pattern_within_match(matched: &str) -> bool {
    SECRET_SKIP_PATTERNS.iter().any(|p| matched.contains(p))
}

pub(super) fn detect_secret_patterns(content: &str) -> Vec<Problem> {
    let patterns = &*SECRET_PATTERNS;

    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            patterns.iter().find_map(|pattern| {
                let m = pattern.regex.find(line)?;
                if skip_pattern_within_match(m.as_str()) {
                    return None;
                }
                Some(Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::SecretPattern { hint: pattern.hint },
                })
            })
        })
        .collect()
}

/// Replace every line matching a secret pattern with a hint-only placeholder.
///
/// The checker deliberately reports secrets hint-only (the matched value never
/// enters a `Problem`); diff output must honor the same contract, including
/// unchanged context lines, so raw values never reach CI logs (issue #44).
///
/// Masking ignores `SECRET_SKIP_PATTERNS` entirely (unlike `detect_secret_patterns`):
/// a skip pattern only decides whether a match is *reported*, never whether a
/// diff shows raw secret-shaped text — an unrelated trailing comment (or any
/// other skip-pattern occurrence) must not un-mask a real secret (issue #77).
pub(super) fn mask_secret_lines(content: &str) -> String {
    let patterns = &*SECRET_PATTERNS;
    let mut out = String::with_capacity(content.len());

    for line in content.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        let hint = patterns
            .iter()
            .find(|p| p.regex.is_match(text))
            .map(|p| p.hint);

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
        let (todos, _) = detect_todo_and_fixme_comments("// TODO: fix this\n");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].line, 1);
    }

    #[test]
    fn test_todo_case_insensitive() {
        assert_eq!(detect_todo_and_fixme_comments("// todo: fix\n").0.len(), 1);
        assert_eq!(detect_todo_and_fixme_comments("// Todo fix\n").0.len(), 1);
    }

    #[test]
    fn test_todo_requires_word_boundary() {
        assert!(detect_todo_and_fixme_comments("use todoist;\n")
            .0
            .is_empty());
    }

    #[test]
    fn test_fixme_detected() {
        let (_, fixmes) = detect_todo_and_fixme_comments("# FIXME: broken\n");
        assert_eq!(fixmes.len(), 1);
    }

    #[test]
    fn test_todo_and_fixme_single_pass_separates_kinds() {
        let (todos, fixmes) =
            detect_todo_and_fixme_comments("// TODO: first\n// FIXME: second\n// TODO: third\n");
        assert_eq!(todos.iter().map(|p| p.line).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(fixmes.iter().map(|p| p.line).collect::<Vec<_>>(), [2]);
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
        // "eprintln!(" contains "println!(" as a substring, but not at a word
        // boundary (preceded by 'e'), so non-strict mode must not flag it.
        assert!(detect_debug_code("eprintln!(\"fail\");\n", false).is_empty());
        assert_eq!(detect_debug_code("eprintln!(\"fail\");\n", true).len(), 1);
    }

    #[test]
    fn test_debug_print_requires_word_boundary() {
        assert!(detect_debug_code("sprint(x);\n", false).is_empty());
        assert!(detect_debug_code("pprint(x);\n", false).is_empty());
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

    // issue #77: a skip pattern trailing a real secret as an unrelated
    // comment must not bypass detection or diff masking.
    #[test]
    fn test_secret_trailing_comment_does_not_bypass_detection() {
        let problems = detect_secret_patterns("password = \"hunter2hunter2\"  # ${\n");
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::SecretPattern { hint } if *hint == "hardcoded secret"
        ));

        let problems =
            detect_secret_patterns("aws_access_key_id = \"AKIAQWERTYUIOPASDFGH\"  // {{\n");
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn test_secret_trailing_comment_still_masked() {
        let masked = mask_secret_lines("password = \"hunter2hunter2\"  # ${\n");
        assert!(!masked.contains("hunter2hunter2"));
        assert!(masked.contains("[line masked: potential hardcoded secret]"));
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
        let line = "ああああああ\n";
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
    fn test_mask_secret_lines_leaves_non_matching_line_untouched() {
        // Unquoted, so it never matches the "hardcoded secret" value pattern in
        // the first place — this is not exercising skip-pattern behavior.
        let content = "password = process.env.PASSWORD\n";
        assert_eq!(mask_secret_lines(content), content);
    }

    // issue #77: masking must never depend on SECRET_SKIP_PATTERNS — even a
    // line that legitimately matches a skip pattern (and is therefore not
    // *reported*) still gets masked if it also matches a secret regex.
    #[test]
    fn test_mask_secret_lines_masks_even_when_skip_pattern_present() {
        let content = "api_key = \"process.env.API_KEY\"\n";
        assert!(detect_secret_patterns(content).is_empty());
        let masked = mask_secret_lines(content);
        assert!(!masked.contains("process.env.API_KEY"));
        assert!(masked.contains("[line masked: potential hardcoded secret]"));
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
