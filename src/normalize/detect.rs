use std::borrow::Cow;
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
    let m = marker.as_bytes();
    debug_assert!(!m.is_empty(), "markers are non-empty constants");
    // Kept in release builds too: `windows(0)` panics, and an empty marker
    // must not match every line.
    if m.is_empty() {
        return false;
    }
    bytes.windows(m.len()).enumerate().any(|(i, w)| {
        w.eq_ignore_ascii_case(m)
            && matches!(
                bytes.get(i + m.len()),
                Some(b':' | b' ' | b'\t' | b'(') | None
            )
    })
}

/// Detects TODO and FIXME markers in a single pass over `content`'s lines
/// (each line is checked for both markers instead of scanning the file twice).
/// Returns the two problem kinds in separate vecs, each in line order, so
/// callers that gate TODO/FIXME detection independently can extend their
/// combined problem list with either or both without reordering results.
pub(super) fn detect_todo_and_fixme_comments(
    content: &str,
    line_map: &[usize],
) -> (Vec<Problem>, Vec<Problem>) {
    let mut todos = Vec::new();
    let mut fixmes = Vec::new();

    for (line_idx, line) in content.lines().enumerate() {
        // Text after a fini:ignore directive is kind-list syntax, not code;
        // never scan it for markers (issue #95).
        let line = super::ignore::directive_start(line).map_or(line, |pos| &line[..pos]);
        if is_valid_marker(line, "TODO") {
            todos.push(Problem {
                line: line_map[line_idx],
                kind: ProblemKind::TodoComment,
            });
        }
        if is_valid_marker(line, "FIXME") {
            fixmes.push(Problem {
                line: line_map[line_idx],
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
    let p = pattern.as_bytes();
    if p.is_empty() {
        return false;
    }
    bytes.windows(p.len()).enumerate().any(|(i, w)| {
        w == p
            && !matches!(
                i.checked_sub(1).map(|j| bytes[j]),
                Some(b) if b.is_ascii_alphanumeric() || b == b'_'
            )
    })
}

pub(super) fn detect_debug_code(
    content: &str,
    strict_mode: bool,
    line_map: &[usize],
) -> Vec<Problem> {
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
                    line: line_map[line_idx],
                    kind: ProblemKind::DebugCode {
                        pattern: pattern.trim_end_matches('('),
                    },
                })
        })
        .collect()
}

/// A skip pattern only excuses a match when it occurs within the matched text
/// itself — e.g. `api_key = "process.env.API_KEY"`, a placeholder — not when
/// it sits outside the match, such as an unrelated trailing comment
/// (`password = "hunter2hunter2"  # ${`). Checking the whole line let a skip
/// marker anywhere on the line defeat detection entirely (issue #77).
fn skip_pattern_within_match(matched: &str) -> bool {
    SECRET_SKIP_PATTERNS.iter().any(|p| matched.contains(p))
}

pub(super) fn detect_secret_patterns(content: &str, line_map: &[usize]) -> Vec<Problem> {
    let patterns = &*SECRET_PATTERNS;

    content
        .lines()
        .enumerate()
        .filter_map(|(line_idx, line)| {
            // Same raw + stripped match as `mask_secret_lines`, so
            // `--keep-zero-width` can't hide a split secret from the check
            // while the diff masks it.
            let stripped = strip_mask_ignorable(line);
            patterns.iter().find_map(|pattern| {
                // find_iter (not find): a skipped placeholder match must not
                // short-circuit past a later, unrelated real secret matching
                // the same pattern on the same line.
                let reportable = |text: &str| {
                    pattern
                        .regex
                        .find_iter(text)
                        .any(|m| !skip_pattern_within_match(m.as_str()))
                };
                (reportable(line) || reportable(&stripped)).then(|| Problem {
                    line: line_map[line_idx],
                    kind: ProblemKind::SecretPattern { hint: pattern.hint },
                })
            })
        })
        .collect()
}

/// Default_Ignorable_Code_Point members plus control characters (C0 except
/// `\t\n\r`, DEL, C1, U+2028/U+2029) that render as nothing or as blank, so
/// they can split a secret invisibly. A superset of `fix::ZERO_WIDTH_CHARS`
/// (enforced by a test). Stripping can switch a match off (U+000B/000C/0085/
/// 2028/2029 are regex `\s`, which `bearer\s+` needs), so callers match the raw
/// line as well as the stripped one: stripping can only add matches. Not `char::is_control()`: that would also
/// strip `\t`, merging tokens a tab separates.
fn is_mask_ignorable(c: char) -> bool {
    matches!(c,
        '\u{00}'..='\u{08}' | '\u{0B}'..='\u{0C}' | '\u{0E}'..='\u{1F}' | '\u{7F}'..='\u{9F}'
        | '\u{2028}' | '\u{2029}'
        | '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}'
        | '\u{17B4}' | '\u{17B5}' | '\u{180B}'..='\u{180F}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}' | '\u{3164}' | '\u{FE00}'..='\u{FE0F}'
        | '\u{FEFF}' | '\u{FFA0}' | '\u{FFF0}'..='\u{FFF8}'
        | '\u{1BCA0}'..='\u{1BCA3}' | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0000}'..='\u{E0FFF}')
}

fn strip_mask_ignorable(text: &str) -> Cow<'_, str> {
    if text.contains(is_mask_ignorable) {
        Cow::Owned(text.chars().filter(|&c| !is_mask_ignorable(c)).collect())
    } else {
        Cow::Borrowed(text)
    }
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
///
/// Invisible characters are stripped before the match decision (a BOM at
/// position 0 included), regardless of `--keep-zero-width`, so they cannot
/// split a secret and leak the raw line (issue #156).
pub(super) fn mask_secret_lines(content: &str) -> String {
    let patterns = &*SECRET_PATTERNS;
    let mut out = String::with_capacity(content.len());

    for line in content.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        let stripped = strip_mask_ignorable(text);
        let hint = patterns
            .iter()
            .find(|p| p.regex.is_match(text) || p.regex.is_match(&stripped))
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

pub(super) fn check_line_length(
    content: &str,
    max_length: usize,
    line_map: &[usize],
) -> Vec<Problem> {
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
                line: line_map[line_idx],
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
    use crate::normalize::identity_line_map;

    #[test]
    fn debug_patterns_are_non_empty() {
        // contains_word_boundary treats an empty pattern as "never matches",
        // which would silently disable a pattern rather than flag every line.
        for pattern in DEBUG_PATTERNS.iter().chain(STRICT_DEBUG_EXTRA) {
            assert!(!pattern.is_empty(), "empty debug pattern");
        }
    }

    #[test]
    fn test_todo_basic() {
        let content = "// TODO: fix this\n";
        let (todos, _) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].line, 1);
    }

    #[test]
    fn test_todo_case_insensitive() {
        let content = "// todo: fix\n";
        assert_eq!(
            detect_todo_and_fixme_comments(content, &identity_line_map(content))
                .0
                .len(),
            1
        );
        let content = "// Todo fix\n";
        assert_eq!(
            detect_todo_and_fixme_comments(content, &identity_line_map(content))
                .0
                .len(),
            1
        );
    }

    #[test]
    fn test_todo_requires_word_boundary() {
        let content = "use todoist;\n";
        assert!(
            detect_todo_and_fixme_comments(content, &identity_line_map(content))
                .0
                .is_empty()
        );
    }

    #[test]
    fn test_fixme_detected() {
        let content = "# FIXME: broken\n";
        let (_, fixmes) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert_eq!(fixmes.len(), 1);
    }

    #[test]
    fn test_todo_and_fixme_single_pass_separates_kinds() {
        let content = "// TODO: first\n// FIXME: second\n// TODO: third\n";
        let (todos, fixmes) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert_eq!(todos.iter().map(|p| p.line).collect::<Vec<_>>(), [1, 3]);
        assert_eq!(fixmes.iter().map(|p| p.line).collect::<Vec<_>>(), [2]);
    }

    // issue #95: a kind name in a fini:ignore(-next-line) directive's own
    // kind-list text must never be mistaken for a real TODO/FIXME marker,
    // but a real marker before the directive on the same line must still
    // be detected.
    #[test]
    fn test_issue_95_directive_kind_list_not_detected_as_marker() {
        let content = "// fini:ignore-next-line todo\n";
        let (todos, _) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert!(todos.is_empty());

        let content = "// TODO: x fini:ignore todo\n";
        let (todos, _) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert_eq!(todos.len(), 1);
    }

    #[test]
    fn test_debug_console_log() {
        let content = "console.log('test');\n";
        let problems = detect_debug_code(content, false, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::DebugCode { pattern } if *pattern == "console.log"
        ));
    }

    #[test]
    fn test_debug_dbg_macro() {
        let content = "dbg!(value);\n";
        let problems = detect_debug_code(content, false, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
    }

    #[test]
    fn test_debug_strict_includes_console_error() {
        let content = "console.error('fail');\n";
        assert!(detect_debug_code(content, false, &identity_line_map(content)).is_empty());
        assert_eq!(
            detect_debug_code(content, true, &identity_line_map(content)).len(),
            1
        );
    }

    #[test]
    fn test_debug_strict_includes_eprintln() {
        // "eprintln!(" contains "println!(" as a substring, but not at a word
        // boundary (preceded by 'e'), so non-strict mode must not flag it.
        let content = "eprintln!(\"fail\");\n";
        assert!(detect_debug_code(content, false, &identity_line_map(content)).is_empty());
        assert_eq!(
            detect_debug_code(content, true, &identity_line_map(content)).len(),
            1
        );
    }

    #[test]
    fn test_debug_print_requires_word_boundary() {
        let content = "sprint(x);\n";
        assert!(detect_debug_code(content, false, &identity_line_map(content)).is_empty());
        let content = "pprint(x);\n";
        assert!(detect_debug_code(content, false, &identity_line_map(content)).is_empty());
    }

    #[test]
    fn test_secret_bearer_token() {
        let content = "Authorization: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::SecretPattern { hint } if *hint == "bearer token"
        ));
    }

    #[test]
    fn test_secret_github_token() {
        let content = "token = \"ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmn\"\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
    }

    // Quoted so each input matches the "hardcoded secret" regex; only a skip
    // pattern inside the match suppresses it. `std::env` can't appear in a
    // match (no SECRET_PATTERNS match can contain `::`, `<`, `{` or `[`), nor
    // can the placeholder markers (`<your-`, `${`, `{{`, `ENV[`); those
    // entries are unreachable, so `test_placeholder_value_not_detected` pins
    // that no regex matches such placeholders, plus the end-to-end result.
    #[test]
    fn test_secret_skip_env_var_reference() {
        for content in [
            "api_key = \"os.environ.API_KEY\"\n",
            "password = \"getenv.PASSWORD\"\n",
        ] {
            assert!(
                detect_secret_patterns(content, &identity_line_map(content)).is_empty(),
                "{content:?}"
            );
        }
    }

    // A quoted value that itself matches a skip pattern is skipped from detection.
    #[test]
    fn test_secret_skip_quoted_env_reference() {
        let content = "api_key = \"process.env.API_KEY\"\n";
        assert!(detect_secret_patterns(content, &identity_line_map(content)).is_empty());
    }

    // Placeholder values never match the secret regexes (asserted directly,
    // then end-to-end), so the skip entries for them stay unreachable.
    #[test]
    fn test_placeholder_value_not_detected() {
        for content in [
            "api_key = \"<your-api-key>\"\n",
            "api_key = \"${API_TOKEN}\"\n",
            "api_key = \"{{API_TOKEN}}\"\n",
            "api_key = \"ENV[API_TOKEN]\"\n",
        ] {
            assert!(
                detect_secret_patterns(content, &identity_line_map(content)).is_empty(),
                "{content:?}"
            );
            assert!(
                SECRET_PATTERNS.iter().all(|p| !p.regex.is_match(content)),
                "placeholder now matches a secret regex; skip entries are reachable again: {content:?}"
            );
        }
    }

    // issue #229: U+2028 is regex `\s` yet stripped as mask-ignorable, so the
    // bearer pattern only matches the raw line.
    #[test]
    fn test_bearer_with_line_separator_masked_and_detected() {
        let content = "Authorization: Bearer\u{2028}abcdefghijklmnopqrstuvwx\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::SecretPattern { hint } if *hint == "bearer token"
        ));
        assert_eq!(
            mask_secret_lines(content),
            "[line masked: potential bearer token]\n"
        );
    }

    // issue #77: a skipped placeholder occurrence must not shadow a later,
    // unrelated real secret matching the same pattern on the same line.
    // Target mutation: `.find_iter` -> `.find` in detect_secret_patterns.
    #[test]
    fn test_secret_skipped_match_does_not_shadow_later_secret() {
        let content = "api_key = \"process.env.API_KEY\", password = \"hunter2hunter2\"\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::SecretPattern { hint } if *hint == "hardcoded secret"
        ));
    }

    // issue #77: a skip pattern trailing a real secret as an unrelated
    // comment must not bypass detection or diff masking.
    #[test]
    fn test_secret_trailing_comment_does_not_bypass_detection() {
        let content = "password = \"hunter2hunter2\"  # ${\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
        assert!(matches!(
            &problems[0].kind,
            ProblemKind::SecretPattern { hint } if *hint == "hardcoded secret"
        ));

        let content = "aws_access_key_id = \"AKIAQWERTYUIOPASDFGH\"  // {{\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
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
        let content = "short\n";
        assert!(check_line_length(content, 80, &identity_line_map(content)).is_empty());
    }

    #[test]
    fn test_line_length_at_limit() {
        let line = format!("{}\n", "a".repeat(80));
        assert!(check_line_length(&line, 80, &identity_line_map(&line)).is_empty());
    }

    #[test]
    fn test_line_length_over_limit() {
        let line = format!("{}\n", "a".repeat(81));
        let problems = check_line_length(&line, 80, &identity_line_map(&line));
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
        assert!(check_line_length(line, 6, &identity_line_map(line)).is_empty());
        assert_eq!(
            check_line_length(line, 5, &identity_line_map(line)).len(),
            1
        );
    }

    #[test]
    fn test_multiple_debug_on_same_line_reports_first() {
        let content = "console.log(dbg!(x));\n";
        let problems = detect_debug_code(content, false, &identity_line_map(content));
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
        assert!(detect_secret_patterns(content, &identity_line_map(content)).is_empty());
        let masked = mask_secret_lines(content);
        assert!(!masked.contains("process.env.API_KEY"));
        assert!(masked.contains("[line masked: potential hardcoded secret]"));
    }

    // issue #156: a zero-width char inside the value must not hide the secret.
    #[test]
    fn test_mask_secret_lines_masks_secret_split_by_zero_width() {
        let masked = mask_secret_lines("password = \"hunter\u{200B}2hunter2\"\n");
        assert!(!masked.contains("hunter\u{200B}2hunter2"));
        assert!(!masked.contains("hunter2hunter2"));
        assert!(masked.contains("[line masked: potential hardcoded secret]"));
    }

    #[test]
    fn test_mask_set_is_superset_of_fixer_zero_width_set() {
        for c in crate::normalize::fix::ZERO_WIDTH_CHARS {
            assert!(
                is_mask_ignorable(*c),
                "U+{:04X} missing from mask set",
                *c as u32
            );
        }
    }

    #[test]
    fn test_mask_secret_lines_masks_secret_split_by_other_invisibles() {
        for c in ['\u{00AD}', '\u{2062}', '\u{FE0F}', '\u{2066}', '\u{E0001}'] {
            let line = format!("token = \"ghp_ABCDEFGHIJKLMNOPQRS{c}TUVWXYZabcdefghijklmn\"\n");
            let masked = mask_secret_lines(&line);
            assert!(
                !masked.contains("ghp_"),
                "U+{:04X} leaked: {masked}",
                c as u32
            );
            assert!(masked.contains("[line masked: potential"));
        }
    }

    // issue #213: control characters render as nothing in a terminal too.
    #[test]
    fn test_secret_split_by_control_chars_is_masked_and_detected() {
        for c in [
            '\u{0001}', '\u{0008}', '\u{000B}', '\u{001F}', '\u{007F}', '\u{0085}', '\u{009F}',
            '\u{2028}', '\u{2029}',
        ] {
            let line = format!("token = \"ghp_ABCDEFGHIJKLMNOPQRS{c}TUVWXYZabcdefghijklmn\"\n");
            let masked = mask_secret_lines(&line);
            assert!(
                !masked.contains("ghp_"),
                "U+{:04X} leaked: {masked}",
                c as u32
            );
            assert!(masked.contains("[line masked: potential"));
            let problems = detect_secret_patterns(&line, &identity_line_map(&line));
            assert_eq!(problems.len(), 1, "U+{:04X} not detected", c as u32);
        }
    }

    #[test]
    fn test_is_mask_ignorable_keeps_tab_newline_and_cr() {
        for c in ['\t', '\n', '\r', ' ', 'a'] {
            assert!(
                !is_mask_ignorable(c),
                "U+{:04X} must not be stripped",
                c as u32
            );
        }
    }

    #[test]
    fn test_detect_secret_patterns_sees_through_invisibles_with_correct_line() {
        let content = "ok\naws_access_key_id = AKIA\u{00AD}ABCDEFGHIJKLMNOP\n";
        let problems = detect_secret_patterns(content, &identity_line_map(content));
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].line, 2);
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

    // issue #75: an earlier invalid substring match (e.g. "TODOLIST") must not
    // stop the scan before a later, valid marker on the same line.
    #[test]
    fn test_issue_75() {
        assert!(is_valid_marker("TODOLIST TODO: fix", "TODO"));

        let content = "// TODOLIST TODO: fix this\n";
        let (todos, _) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].line, 1);

        let content = "// FIXMELIST FIXME: broken\n";
        let (_, fixmes) = detect_todo_and_fixme_comments(content, &identity_line_map(content));
        assert_eq!(fixmes.len(), 1);
        assert_eq!(fixmes[0].line, 1);
    }
}
