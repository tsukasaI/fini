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
                        pattern: pattern.trim_end_matches('(').to_string(),
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
                    kind: ProblemKind::SecretPattern {
                        hint: pattern.hint.to_string(),
                    },
                })
        })
        .collect()
}

pub(super) fn check_line_length(content: &str, max_length: usize) -> Vec<Problem> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
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
