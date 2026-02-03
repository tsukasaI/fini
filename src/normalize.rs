/// Full-width space character (U+3000)
const FULLWIDTH_SPACE: char = '\u{3000}';

use serde::{Deserialize, Serialize};

/// Configuration for normalization rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizeConfig {
    /// Maximum consecutive blank lines (None = no limit)
    pub max_blank_lines: Option<usize>,
    /// Remove zero-width characters (default: true)
    pub remove_zero_width: bool,
    /// Remove leading blank lines (default: true)
    pub remove_leading_blanks: bool,
    /// Remove code block remnants (default: false)
    pub fix_code_blocks: bool,
}

impl Default for NormalizeConfig {
    fn default() -> Self {
        Self {
            max_blank_lines: None,
            remove_zero_width: true,
            remove_leading_blanks: true,
            fix_code_blocks: false,
        }
    }
}

/// Normalize file content according to fini rules
pub fn normalize_content(content: &str, config: &NormalizeConfig) -> NormalizeResult {
    let mut result = content.to_string();
    let mut problems = vec![];

    // Line ending normalization (CRLF/CR → LF)
    result = normalize_line_endings(&result);

    // Zero-width character removal (before leading blank removal to track correct positions)
    if config.remove_zero_width {
        let (fixed, zw_problems) = remove_zero_width_chars(&result);
        result = fixed;
        problems.extend(zw_problems);
    }

    // Leading blank lines removal (before other normalizations)
    if config.remove_leading_blanks {
        let (fixed, leading_problems) = remove_leading_blank_lines(&result);
        result = fixed;
        problems.extend(leading_problems);
    }

    // Consecutive blank line limiting (before other normalizations)
    if let Some(max) = config.max_blank_lines {
        let (fixed, blank_problems) = limit_consecutive_blank_lines(&result, max);
        result = fixed;
        problems.extend(blank_problems);
    }

    // Code block remnant removal (opt-in)
    if config.fix_code_blocks {
        let (fixed, code_block_problems) = remove_code_block_remnants(&result);
        result = fixed;
        problems.extend(code_block_problems);
    }

    // Full-width space detection and fix
    let (fixed, fullwidth_problems) = fix_fullwidth_spaces(&result);
    result = fixed;
    problems.extend(fullwidth_problems);

    // Trailing whitespace removal
    result = remove_trailing_whitespace(&result);

    // EOF newline normalization
    result = normalize_eof_newline(&result);

    NormalizeResult {
        original: content.to_string(),
        content: result,
        problems,
    }
}

fn normalize_line_endings(content: &str) -> String {
    // First convert CRLF to LF, then CR to LF
    content.replace("\r\n", "\n").replace('\r', "\n")
}

fn fix_fullwidth_spaces(content: &str) -> (String, Vec<Problem>) {
    let problems: Vec<Problem> = content
        .lines()
        .enumerate()
        .flat_map(|(line_idx, line)| {
            let count = line.chars().filter(|&c| c == FULLWIDTH_SPACE).count();
            std::iter::repeat_n(
                Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::FullWidthSpace,
                },
                count,
            )
        })
        .collect();

    let result = content.replace(FULLWIDTH_SPACE, " ");
    (result, problems)
}

fn remove_trailing_whitespace(content: &str) -> String {
    content
        .lines()
        .map(|line| line.trim_end_matches([' ', '\t']))
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_eof_newline(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    let trimmed = content.trim_end_matches('\n');
    format!("{trimmed}\n")
}

fn remove_leading_blank_lines(content: &str) -> (String, Vec<Problem>) {
    let lines: Vec<&str> = content.lines().collect();
    let first_non_blank = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());

    let problems = if first_non_blank > 0 {
        vec![Problem {
            line: 1,
            kind: ProblemKind::LeadingBlankLines {
                count: first_non_blank,
            },
        }]
    } else {
        vec![]
    };

    // All lines are blank if first_non_blank >= lines.len()
    let result = lines
        .get(first_non_blank..)
        .map_or(String::new(), |rest| rest.join("\n"));

    (result, problems)
}

fn limit_consecutive_blank_lines(content: &str, max: usize) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result_lines = vec![];
    let mut blank_count = 0;
    let mut problem_start_line = 0;

    for (line_idx, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= max {
                result_lines.push(line);
            } else if blank_count == max + 1 {
                // Record the start of excessive blank lines
                problem_start_line = line_idx + 1;
            }
        } else {
            if blank_count > max {
                // Record the problem
                problems.push(Problem {
                    line: problem_start_line,
                    kind: ProblemKind::ExcessiveBlankLines {
                        found: blank_count,
                        limit: max,
                    },
                });
            }
            blank_count = 0;
            result_lines.push(line);
        }
    }

    // Handle trailing blank lines
    if blank_count > max {
        problems.push(Problem {
            line: problem_start_line,
            kind: ProblemKind::ExcessiveBlankLines {
                found: blank_count,
                limit: max,
            },
        });
    }

    (result_lines.join("\n"), problems)
}

fn remove_code_block_remnants(content: &str) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result_lines = vec![];

    for (line_idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Check if this line looks like a markdown code fence
        // Valid code fences: ```, ```rust, ```python, ``` (with trailing space)
        if let Some(after_backticks) = trimmed.strip_prefix("```") {
            // A valid fence has nothing or just a language identifier after the backticks
            // Language identifiers are alphanumeric with optional - or +
            let is_valid_fence = after_backticks.is_empty()
                || after_backticks
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '+' || c.is_whitespace());

            if is_valid_fence {
                problems.push(Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::CodeBlockRemnant,
                });
                // Skip this line (don't add to result)
                continue;
            }
        }

        result_lines.push(line);
    }

    (result_lines.join("\n"), problems)
}

/// Zero-width characters to remove (except BOM at file start)
const ZERO_WIDTH_CHARS: &[char] = &[
    '\u{200B}', // Zero Width Space (ZWSP)
    '\u{200C}', // Zero Width Non-Joiner (ZWNJ)
    '\u{200D}', // Zero Width Joiner (ZWJ)
    '\u{200E}', // Left-to-Right Mark
    '\u{200F}', // Right-to-Left Mark
    '\u{2060}', // Word Joiner
    '\u{FEFF}', // Byte Order Mark (BOM) - removed except at file start
];

fn remove_zero_width_chars(content: &str) -> (String, Vec<Problem>) {
    let mut problems = vec![];
    let mut result = String::with_capacity(content.len());
    let mut char_idx = 0;

    for (line_idx, line) in content.lines().enumerate() {
        for ch in line.chars() {
            let is_zero_width = ZERO_WIDTH_CHARS.contains(&ch);
            let is_bom_at_start = ch == '\u{FEFF}' && char_idx == 0;

            if is_zero_width && !is_bom_at_start {
                problems.push(Problem {
                    line: line_idx + 1,
                    kind: ProblemKind::ZeroWidthCharacter,
                });
            } else {
                result.push(ch);
            }
            char_idx += 1;
        }
        result.push('\n');
        char_idx += 1; // for the newline
    }

    // Remove the trailing newline we added (EOF normalization handles this)
    if result.ends_with('\n') && !content.ends_with('\n') {
        result.pop();
    }

    (result, problems)
}

#[derive(Debug, Clone)]
pub struct NormalizeResult {
    pub original: String,
    pub content: String,
    pub problems: Vec<Problem>,
}

impl NormalizeResult {
    pub fn has_changes(&self) -> bool {
        self.original != self.content
    }
}

#[derive(Debug, Clone)]
pub struct Problem {
    pub line: usize,
    pub kind: ProblemKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProblemKind {
    FullWidthSpace,
    LeadingBlankLines { count: usize },
    ZeroWidthCharacter,
    ExcessiveBlankLines { found: usize, limit: usize },
    CodeBlockRemnant,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================
    // Phase 1.1: EOF Newline Normalization
    // ===========================================

    #[test]
    fn test_add_eof_newline_when_missing() {
        let input = "hello";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_no_change_when_eof_newline_exists() {
        let input = "hello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_normalize_multiple_trailing_newlines() {
        let input = "hello\n\n\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_normalize_multiple_trailing_newlines_with_content() {
        let input = "line1\nline2\n\n\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\nline2\n");
    }

    // ===========================================
    // Phase 1.2: Line Ending Normalization
    // ===========================================

    #[test]
    fn test_crlf_to_lf() {
        let input = "line1\r\nline2\r\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\nline2\n");
    }

    #[test]
    fn test_cr_only_to_lf() {
        let input = "line1\rline2\r";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\nline2\n");
    }

    #[test]
    fn test_mixed_line_endings() {
        let input = "line1\r\nline2\rline3\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_lf_unchanged() {
        let input = "line1\nline2\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\nline2\n");
    }

    // ===========================================
    // Phase 1.3: Trailing Whitespace Removal
    // ===========================================

    #[test]
    fn test_remove_trailing_spaces() {
        let input = "hello   \nworld  \n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\nworld\n");
    }

    #[test]
    fn test_remove_trailing_tabs() {
        let input = "hello\t\t\nworld\t\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\nworld\n");
    }

    #[test]
    fn test_preserve_blank_lines() {
        let input = "line1\n\nline2\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\n\nline2\n");
    }

    #[test]
    fn test_preserve_indentation() {
        let input = "    indented\n\tTabbed\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "    indented\n\tTabbed\n");
    }

    #[test]
    fn test_mixed_trailing_whitespace() {
        let input = "hello  \t \nworld\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\nworld\n");
    }

    // ===========================================
    // Phase 1.4: Full-width Space Detection/Fix
    // ===========================================

    #[test]
    fn test_detect_fullwidth_space() {
        let input = "hello\u{3000}world\n"; // U+3000 is full-width space
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::FullWidthSpace));
    }

    #[test]
    fn test_fix_fullwidth_space() {
        let input = "hello\u{3000}world\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello world\n");
    }

    #[test]
    fn test_report_fullwidth_space_line_number() {
        let input = "line1\nline2\u{3000}here\nline3\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| p.kind == ProblemKind::FullWidthSpace);
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 2);
    }

    #[test]
    fn test_multiple_fullwidth_spaces() {
        let input = "a\u{3000}b\u{3000}c\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "a b c\n");
        assert_eq!(
            result
                .problems
                .iter()
                .filter(|p| p.kind == ProblemKind::FullWidthSpace)
                .count(),
            2
        );
    }

    // ===========================================
    // has_changes() tests
    // ===========================================

    #[test]
    fn test_has_changes_when_content_modified() {
        let input = "hello"; // missing EOF newline
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result.has_changes());
    }

    #[test]
    fn test_no_changes_when_content_already_normalized() {
        let input = "hello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(!result.has_changes());
    }

    #[test]
    fn test_has_changes_with_trailing_whitespace() {
        let input = "hello   \n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result.has_changes());
    }

    // ===========================================
    // Leading Blank Lines Removal
    // ===========================================

    #[test]
    fn test_remove_leading_blank_lines() {
        let input = "\n\n\nhello\nworld\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\nworld\n");
    }

    #[test]
    fn test_single_leading_blank_line() {
        let input = "\nhello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_no_leading_blank_lines_unchanged() {
        let input = "hello\nworld\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\nworld\n");
    }

    #[test]
    fn test_keep_leading_blanks_when_disabled() {
        let config = NormalizeConfig {
            remove_leading_blanks: false,
            ..NormalizeConfig::default()
        };
        let input = "\n\nhello\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "\n\nhello\n");
    }

    #[test]
    fn test_leading_blank_problem_reports_count() {
        let input = "\n\n\nhello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::LeadingBlankLines { .. }));
        assert!(problem.is_some());
        if let ProblemKind::LeadingBlankLines { count } = problem.unwrap().kind {
            assert_eq!(count, 3);
        }
    }

    // ===========================================
    // Zero-width Character Removal
    // ===========================================

    #[test]
    fn test_remove_zwsp() {
        let input = "hello\u{200B}world\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "helloworld\n");
    }

    #[test]
    fn test_remove_zwj() {
        let input = "a\u{200D}b\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "ab\n");
    }

    #[test]
    fn test_remove_zwnj() {
        let input = "a\u{200C}b\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "ab\n");
    }

    #[test]
    fn test_preserve_bom_at_file_start() {
        let input = "\u{FEFF}hello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "\u{FEFF}hello\n");
    }

    #[test]
    fn test_remove_bom_in_middle_of_file() {
        let input = "hello\u{FEFF}world\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "helloworld\n");
    }

    #[test]
    fn test_keep_zero_width_when_disabled() {
        let config = NormalizeConfig {
            remove_zero_width: false,
            ..NormalizeConfig::default()
        };
        let input = "hello\u{200B}world\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "hello\u{200B}world\n");
    }

    #[test]
    fn test_zero_width_problem_reports_line() {
        let input = "line1\nline2\u{200B}here\nline3\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::ZeroWidthCharacter));
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 2);
    }

    #[test]
    fn test_multiple_zero_width_chars() {
        let input = "a\u{200B}b\u{200D}c\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "abc\n");
        assert_eq!(
            result
                .problems
                .iter()
                .filter(|p| matches!(p.kind, ProblemKind::ZeroWidthCharacter))
                .count(),
            2
        );
    }

    // ===========================================
    // Consecutive Blank Line Limit
    // ===========================================

    #[test]
    fn test_limit_blank_lines_to_2() {
        let config = NormalizeConfig {
            max_blank_lines: Some(2),
            ..NormalizeConfig::default()
        };
        let input = "line1\n\n\n\n\nline2\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "line1\n\n\nline2\n");
    }

    #[test]
    fn test_blank_lines_under_limit_unchanged() {
        let config = NormalizeConfig {
            max_blank_lines: Some(2),
            ..NormalizeConfig::default()
        };
        let input = "line1\n\nline2\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "line1\n\nline2\n");
    }

    #[test]
    fn test_limit_blank_lines_to_1() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            ..NormalizeConfig::default()
        };
        let input = "line1\n\n\nline2\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "line1\n\nline2\n");
    }

    #[test]
    fn test_limit_blank_lines_to_0() {
        let config = NormalizeConfig {
            max_blank_lines: Some(0),
            ..NormalizeConfig::default()
        };
        let input = "line1\n\nline2\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "line1\nline2\n");
    }

    #[test]
    fn test_no_limit_by_default() {
        let input = "line1\n\n\n\n\nline2\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\n\n\n\n\nline2\n");
    }

    #[test]
    fn test_excessive_blank_lines_problem_reports() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            ..NormalizeConfig::default()
        };
        let input = "line1\n\n\n\nline2\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::ExcessiveBlankLines { .. }));
        assert!(problem.is_some());
        if let ProblemKind::ExcessiveBlankLines { found, limit } = problem.unwrap().kind {
            assert_eq!(found, 3);
            assert_eq!(limit, 1);
        }
    }

    #[test]
    fn test_multiple_excessive_blank_line_groups() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            ..NormalizeConfig::default()
        };
        let input = "a\n\n\n\nb\n\n\nc\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "a\n\nb\n\nc\n");
        assert_eq!(
            result
                .problems
                .iter()
                .filter(|p| matches!(p.kind, ProblemKind::ExcessiveBlankLines { .. }))
                .count(),
            2
        );
    }

    // ===========================================
    // Code Block Remnant Removal
    // ===========================================

    #[test]
    fn test_remove_code_fence_opening() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        let input = "```rust\nfn main() {}\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "fn main() {}\n");
    }

    #[test]
    fn test_remove_code_fence_closing() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        let input = "fn main() {}\n```\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "fn main() {}\n");
    }

    #[test]
    fn test_remove_code_fence_both() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        let input = "```rust\nfn main() {}\n```\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "fn main() {}\n");
    }

    #[test]
    fn test_no_false_positive_backticks_in_string() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        // This should NOT be removed because it's not a valid fence pattern
        let input = "let s = \"use ```code``` blocks\";\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "let s = \"use ```code``` blocks\";\n");
    }

    #[test]
    fn test_code_block_disabled_by_default() {
        let input = "```rust\nfn main() {}\n```\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn test_code_block_problem_reports_line() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        let input = "line1\n```rust\ncode\n```\nline2\n";
        let result = normalize_content(input, &config);
        let problems: Vec<_> = result
            .problems
            .iter()
            .filter(|p| matches!(p.kind, ProblemKind::CodeBlockRemnant))
            .collect();
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0].line, 2); // ```rust
        assert_eq!(problems[1].line, 4); // ```
    }

    #[test]
    fn test_code_fence_with_language_variants() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        // Test various language identifiers
        for lang in &["python", "javascript", "c++", "c-sharp", ""] {
            let input = format!("```{}\ncode\n", lang);
            let result = normalize_content(&input, &config);
            assert_eq!(result.content, "code\n", "Failed for lang: {}", lang);
        }
    }

    // ===========================================
    // Edge Cases: Leading Blank Lines
    // ===========================================

    #[test]
    fn test_file_with_only_blank_lines() {
        let input = "\n\n\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        // All blank lines removed, empty file gets no EOF newline
        assert_eq!(result.content, "");
    }

    #[test]
    fn test_whitespace_only_lines_at_start() {
        // Lines with only spaces/tabs should be treated as blank
        let input = "   \n\t\n  \t  \nhello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_empty_file_unchanged() {
        let input = "";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "");
        assert!(!result.has_changes());
    }

    // ===========================================
    // Edge Cases: Zero-width Characters
    // ===========================================

    #[test]
    fn test_zero_width_at_start_of_line() {
        let input = "\u{200B}hello\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_zero_width_at_end_of_line() {
        let input = "hello\u{200B}\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_bom_on_second_line_removed() {
        // BOM should only be preserved at very start of file
        let input = "line1\n\u{FEFF}line2\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "line1\nline2\n");
    }

    #[test]
    fn test_consecutive_zero_width_chars() {
        let input = "a\u{200B}\u{200D}\u{200C}b\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "ab\n");
        assert_eq!(
            result
                .problems
                .iter()
                .filter(|p| matches!(p.kind, ProblemKind::ZeroWidthCharacter))
                .count(),
            3
        );
    }

    // ===========================================
    // Edge Cases: Consecutive Blank Lines
    // ===========================================

    #[test]
    fn test_blank_lines_at_end_of_file() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            remove_leading_blanks: false,
            ..NormalizeConfig::default()
        };
        // Trailing blank lines are handled by EOF normalization, not blank line limit
        let input = "hello\n\n\n\n";
        let result = normalize_content(input, &config);
        // EOF normalization reduces to single newline
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_whitespace_lines_count_as_blank_for_limit() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            ..NormalizeConfig::default()
        };
        let input = "a\n   \n\t\n  \nb\n";
        let result = normalize_content(input, &config);
        // Whitespace-only lines count as blank
        assert_eq!(result.content, "a\n\nb\n");
    }

    #[test]
    fn test_blank_limit_with_leading_removal_interaction() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            remove_leading_blanks: true,
            ..NormalizeConfig::default()
        };
        let input = "\n\n\na\n\n\n\nb\n";
        let result = normalize_content(input, &config);
        // Leading blanks removed first, then blank limit applied
        assert_eq!(result.content, "a\n\nb\n");
    }

    // ===========================================
    // Edge Cases: Code Block Remnants
    // ===========================================

    #[test]
    fn test_indented_code_fence() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        // Indented code fences should also be detected
        let input = "  ```rust\ncode\n  ```\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "code\n");
    }

    #[test]
    fn test_code_fence_with_numbers_not_removed() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        // Numbers after ``` are valid language identifiers
        let input = "```123\ncode\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "code\n");
    }

    #[test]
    fn test_backticks_with_content_before_not_removed() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        // Backticks with content before should not be removed
        let input = "some text ```\ncode\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "some text ```\ncode\n");
    }

    #[test]
    fn test_four_backticks_not_removed() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        // Four backticks is a different fence type, not caught by ``` detection
        // After stripping ```, we get `rust which contains a backtick
        let input = "````rust\ncode\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "````rust\ncode\n");
    }

    // ===========================================
    // Edge Cases: Combined Features
    // ===========================================

    #[test]
    fn test_all_features_combined() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            remove_zero_width: true,
            remove_leading_blanks: true,
            fix_code_blocks: true,
        };
        let input = "\n\n```rust\nfn main() {\n    let x\u{200B} = 1;\n\n\n\n}\n```\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "fn main() {\n    let x = 1;\n\n}\n");
    }

    #[test]
    fn test_zero_width_in_code_fence_line() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            remove_zero_width: true,
            ..NormalizeConfig::default()
        };
        // Zero-width chars are removed first, then code fence detection
        let input = "```\u{200B}rust\ncode\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "code\n");
    }
}
