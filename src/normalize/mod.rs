mod detect;
mod fix;
mod ignore;

use serde::{Deserialize, Serialize};

use detect::{
    check_line_length, detect_debug_code, detect_fixme_comments, detect_secret_patterns,
    detect_todo_comments,
};
use fix::{
    fix_fullwidth_spaces, limit_consecutive_blank_lines, normalize_eof_newline,
    normalize_line_endings, remove_code_block_remnants, remove_leading_blank_lines,
    remove_trailing_whitespace, remove_zero_width_chars,
};

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
    /// Detect TODO comments (default: true)
    pub detect_todos: bool,
    /// Detect FIXME comments (default: true)
    pub detect_fixmes: bool,
    /// Detect debug code like console.log, print() (default: true)
    pub detect_debug: bool,
    /// Include console.error in debug detection (default: false)
    pub strict_debug: bool,
    /// Detect secret patterns like API keys (default: true)
    pub detect_secrets: bool,
    /// Maximum line length (None = disabled)
    pub max_line_length: Option<usize>,
}

impl Default for NormalizeConfig {
    fn default() -> Self {
        Self {
            max_blank_lines: None,
            remove_zero_width: true,
            remove_leading_blanks: true,
            fix_code_blocks: false,
            detect_todos: true,
            detect_fixmes: true,
            detect_debug: true,
            strict_debug: false,
            detect_secrets: true,
            max_line_length: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NormalizeResult {
    pub content: String,
    pub changed: bool,
    pub problems: Vec<Problem>,
}

impl NormalizeResult {
    pub fn has_changes(&self) -> bool {
        self.changed
    }

    pub fn has_detection_problems(&self) -> bool {
        self.problems.iter().any(|p| p.kind.is_detection_only())
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
    TodoComment,
    FixmeComment,
    DebugCode { pattern: &'static str },
    SecretPattern { hint: &'static str },
    LongLine { length: usize, limit: usize },
}

impl ProblemKind {
    /// Returns true if this is a detection-only problem (not auto-fixed)
    pub fn is_detection_only(&self) -> bool {
        matches!(
            self,
            ProblemKind::TodoComment
                | ProblemKind::FixmeComment
                | ProblemKind::DebugCode { .. }
                | ProblemKind::SecretPattern { .. }
                | ProblemKind::LongLine { .. }
        )
    }

    /// Returns the identifier used in `fini:ignore` directives
    pub fn ignore_id(&self) -> &'static str {
        match self {
            ProblemKind::TodoComment => "todo",
            ProblemKind::FixmeComment => "fixme",
            ProblemKind::DebugCode { .. } => "debug",
            ProblemKind::SecretPattern { .. } => "secret",
            ProblemKind::LongLine { .. } => "line-length",
            ProblemKind::FullWidthSpace => "fullwidth",
            ProblemKind::LeadingBlankLines { .. } => "leading-blanks",
            ProblemKind::ZeroWidthCharacter => "zero-width",
            ProblemKind::ExcessiveBlankLines { .. } => "blank-lines",
            ProblemKind::CodeBlockRemnant => "code-block",
        }
    }
}

/// Normalize file content according to fini rules
pub fn normalize_content(content: &str, config: &NormalizeConfig) -> NormalizeResult {
    let mut result = content.to_string();
    let mut problems = vec![];

    result = normalize_line_endings(&result);

    if config.remove_zero_width {
        let (fixed, zw_problems) = remove_zero_width_chars(&result);
        result = fixed;
        problems.extend(zw_problems);
    }

    if config.remove_leading_blanks {
        let (fixed, leading_problems) = remove_leading_blank_lines(&result);
        result = fixed;
        problems.extend(leading_problems);
    }

    if let Some(max) = config.max_blank_lines {
        let (fixed, blank_problems) = limit_consecutive_blank_lines(&result, max);
        result = fixed;
        problems.extend(blank_problems);
    }

    if config.fix_code_blocks {
        let (fixed, code_block_problems) = remove_code_block_remnants(&result);
        result = fixed;
        problems.extend(code_block_problems);
    }

    let (fixed, fullwidth_problems) = fix_fullwidth_spaces(&result);
    result = fixed;
    problems.extend(fullwidth_problems);

    result = remove_trailing_whitespace(&result);
    result = normalize_eof_newline(&result);

    // Detection only (no auto-fix)
    if config.detect_todos {
        problems.extend(detect_todo_comments(&result));
    }

    if config.detect_fixmes {
        problems.extend(detect_fixme_comments(&result));
    }

    if config.detect_debug {
        problems.extend(detect_debug_code(&result, config.strict_debug));
    }

    if config.detect_secrets {
        problems.extend(detect_secret_patterns(&result));
    }

    if let Some(max_length) = config.max_line_length {
        problems.extend(check_line_length(&result, max_length));
    }

    // Filter out problems suppressed by inline ignore directives
    if !problems.is_empty() {
        let ignore_map = ignore::parse_ignore_directives(&result);
        if !ignore_map.is_empty() {
            problems.retain(|p| !ignore_map.is_ignored(p.line, &p.kind));
        }
    }

    let changed = result != content;

    NormalizeResult {
        content: result,
        changed,
        problems,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================
    // EOF Newline Normalization
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
    // Line Ending Normalization
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
    // Trailing Whitespace Removal
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
    // Full-width Space Detection/Fix
    // ===========================================

    #[test]
    fn test_detect_fullwidth_space() {
        let input = "hello\u{3000}world\n";
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
    // has_changes()
    // ===========================================

    #[test]
    fn test_has_changes_when_content_modified() {
        let input = "hello";
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
        assert_eq!(problems[0].line, 2);
        assert_eq!(problems[1].line, 4);
    }

    #[test]
    fn test_code_fence_with_language_variants() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
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
        assert_eq!(result.content, "");
    }

    #[test]
    fn test_whitespace_only_lines_at_start() {
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
        let input = "hello\n\n\n\n";
        let result = normalize_content(input, &config);
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
            ..NormalizeConfig::default()
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
        let input = "```\u{200B}rust\ncode\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "code\n");
    }

    // ===========================================
    // Long Line Detection
    // ===========================================

    #[test]
    fn test_detect_line_over_default_limit() {
        let config = NormalizeConfig {
            max_line_length: Some(120),
            ..NormalizeConfig::default()
        };
        let input = format!("{}\n", "a".repeat(121));
        let result = normalize_content(&input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::LongLine { .. }));
        assert!(problem.is_some());
        if let ProblemKind::LongLine { length, limit } = problem.unwrap().kind {
            assert_eq!(length, 121);
            assert_eq!(limit, 120);
        }
    }

    #[test]
    fn test_no_problem_for_line_at_limit() {
        let config = NormalizeConfig {
            max_line_length: Some(120),
            ..NormalizeConfig::default()
        };
        let input = format!("{}\n", "a".repeat(120));
        let result = normalize_content(&input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::LongLine { .. }));
        assert!(problem.is_none());
    }

    #[test]
    fn test_detect_multiple_long_lines() {
        let config = NormalizeConfig {
            max_line_length: Some(120),
            ..NormalizeConfig::default()
        };
        let input = format!("{}\n{}\n", "a".repeat(150), "b".repeat(130));
        let result = normalize_content(&input, &config);
        let problems: Vec<_> = result
            .problems
            .iter()
            .filter(|p| matches!(p.kind, ProblemKind::LongLine { .. }))
            .collect();
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0].line, 1);
        assert_eq!(problems[1].line, 2);
    }

    #[test]
    fn test_line_length_counts_characters_not_bytes() {
        let config = NormalizeConfig {
            max_line_length: Some(40),
            ..NormalizeConfig::default()
        };
        let input = format!("{}\n", "あ".repeat(41));
        let result = normalize_content(&input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::LongLine { .. }));
        assert!(problem.is_some());
        if let ProblemKind::LongLine { length, limit } = problem.unwrap().kind {
            assert_eq!(length, 41);
            assert_eq!(limit, 40);
        }
    }

    #[test]
    fn test_line_length_disabled_by_default() {
        let input = format!("{}\n", "a".repeat(200));
        let result = normalize_content(&input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::LongLine { .. }));
        assert!(problem.is_none());
    }

    // ===========================================
    // TODO/FIXME Detection
    // ===========================================

    #[test]
    fn test_detect_todo_in_single_line_comment() {
        let input = "// TODO: fix this later\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::TodoComment));
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 1);
    }

    #[test]
    fn test_detect_fixme_in_single_line_comment() {
        let input = "// FIXME: urgent bug\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::FixmeComment));
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 1);
    }

    #[test]
    fn test_detect_todo_case_insensitive() {
        let input = "// todo: lowercase\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::TodoComment));
        assert!(problem.is_some());
    }

    #[test]
    fn test_detect_multiple_todos_in_file() {
        let input = "// TODO: first\ncode\n// TODO: second\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problems: Vec<_> = result
            .problems
            .iter()
            .filter(|p| matches!(p.kind, ProblemKind::TodoComment))
            .collect();
        assert_eq!(problems.len(), 2);
        assert_eq!(problems[0].line, 1);
        assert_eq!(problems[1].line, 3);
    }

    #[test]
    fn test_todo_detection_disabled() {
        let config = NormalizeConfig {
            detect_todos: false,
            ..NormalizeConfig::default()
        };
        let input = "// TODO: fix this\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::TodoComment));
        assert!(problem.is_none());
    }

    #[test]
    fn test_fixme_detection_disabled() {
        let config = NormalizeConfig {
            detect_fixmes: false,
            ..NormalizeConfig::default()
        };
        let input = "// FIXME: urgent\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::FixmeComment));
        assert!(problem.is_none());
    }

    // ===========================================
    // Debug Code Detection
    // ===========================================

    #[test]
    fn test_detect_console_log() {
        let input = "console.log('debug');\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::DebugCode { .. }));
        assert!(problem.is_some());
        if let ProblemKind::DebugCode { pattern } = &problem.unwrap().kind {
            assert_eq!(*pattern, "console.log");
        }
    }

    #[test]
    fn test_detect_console_error_with_strict_mode() {
        let config = NormalizeConfig {
            strict_debug: true,
            ..NormalizeConfig::default()
        };
        let input = "console.error('error');\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::DebugCode { .. }));
        assert!(problem.is_some());
    }

    #[test]
    fn test_console_error_not_detected_by_default() {
        let input = "console.error('error');\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::DebugCode { .. }));
        assert!(problem.is_none());
    }

    #[test]
    fn test_debug_detection_disabled() {
        let config = NormalizeConfig {
            detect_debug: false,
            ..NormalizeConfig::default()
        };
        let input = "console.log('debug');\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::DebugCode { .. }));
        assert!(problem.is_none());
    }

    // ===========================================
    // Secret Pattern Detection
    // ===========================================

    #[test]
    fn test_detect_api_key_pattern() {
        let input = "const API_KEY = \"sk_live_abcd1234\";\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::SecretPattern { .. }));
        assert!(problem.is_some());
    }

    #[test]
    fn test_no_false_positive_for_env_var() {
        let input = "API_KEY = process.env.API_KEY\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::SecretPattern { .. }));
        assert!(problem.is_none());
    }

    #[test]
    fn test_secret_detection_disabled() {
        let config = NormalizeConfig {
            detect_secrets: false,
            ..NormalizeConfig::default()
        };
        let input = "API_KEY = \"sk_live_abcd1234\"\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::SecretPattern { .. }));
        assert!(problem.is_none());
    }

    // ── Inline ignore directives ──

    #[test]
    fn test_ignore_inline_suppresses_todo() {
        let input = "// TODO: fix this fini:ignore\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result.problems.is_empty());
    }

    #[test]
    fn test_ignore_next_line_suppresses_todo() {
        let input = "// fini:ignore-next-line\n// TODO: fix this\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result.problems.is_empty());
    }

    #[test]
    fn test_ignore_selective_debug() {
        let input = "console.log('test'); // fini:ignore debug\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result.problems.is_empty());
    }

    #[test]
    fn test_ignore_selective_does_not_suppress_other() {
        let input = "console.log('test'); // TODO: fix fini:ignore debug\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.problems.len(), 1);
        assert!(matches!(result.problems[0].kind, ProblemKind::TodoComment));
    }

    #[test]
    fn test_ignore_next_line_selective() {
        let config = NormalizeConfig {
            detect_debug: true,
            detect_todos: true,
            ..NormalizeConfig::default()
        };
        let input = "# fini:ignore-next-line debug\nprint('hello') # TODO: later\n";
        let result = normalize_content(input, &config);
        // debug is suppressed, but TODO is not
        assert_eq!(result.problems.len(), 1);
        assert!(matches!(result.problems[0].kind, ProblemKind::TodoComment));
    }

    #[test]
    fn test_ignore_without_directive_still_detects() {
        let input = "// TODO: fix this\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.problems.len(), 1);
        assert!(matches!(result.problems[0].kind, ProblemKind::TodoComment));
    }
}
