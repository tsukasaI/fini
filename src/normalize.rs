/// Full-width space character (U+3000)
const FULLWIDTH_SPACE: char = '\u{3000}';

/// Normalize file content according to fini rules
pub fn normalize_content(content: &str) -> NormalizeResult {
    let mut result = content.to_string();
    let mut problems = vec![];

    // Line ending normalization (CRLF/CR → LF)
    result = normalize_line_endings(&result);

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
    let mut problems = vec![];

    for (line_idx, line) in content.lines().enumerate() {
        let count = line.chars().filter(|&c| c == FULLWIDTH_SPACE).count();
        for _ in 0..count {
            problems.push(Problem {
                line: line_idx + 1,
                kind: ProblemKind::FullWidthSpace,
            });
        }
    }

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
    MissingEofNewline,
    TrailingWhitespace,
    FullWidthSpace,
    CrlfLineEnding,
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
        let result = normalize_content(input);
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_no_change_when_eof_newline_exists() {
        let input = "hello\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_normalize_multiple_trailing_newlines() {
        let input = "hello\n\n\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "hello\n");
    }

    #[test]
    fn test_normalize_multiple_trailing_newlines_with_content() {
        let input = "line1\nline2\n\n\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "line1\nline2\n");
    }

    // ===========================================
    // Phase 1.2: Line Ending Normalization
    // ===========================================

    #[test]
    fn test_crlf_to_lf() {
        let input = "line1\r\nline2\r\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "line1\nline2\n");
    }

    #[test]
    fn test_cr_only_to_lf() {
        let input = "line1\rline2\r";
        let result = normalize_content(input);
        assert_eq!(result.content, "line1\nline2\n");
    }

    #[test]
    fn test_mixed_line_endings() {
        let input = "line1\r\nline2\rline3\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "line1\nline2\nline3\n");
    }

    #[test]
    fn test_lf_unchanged() {
        let input = "line1\nline2\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "line1\nline2\n");
    }

    // ===========================================
    // Phase 1.3: Trailing Whitespace Removal
    // ===========================================

    #[test]
    fn test_remove_trailing_spaces() {
        let input = "hello   \nworld  \n";
        let result = normalize_content(input);
        assert_eq!(result.content, "hello\nworld\n");
    }

    #[test]
    fn test_remove_trailing_tabs() {
        let input = "hello\t\t\nworld\t\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "hello\nworld\n");
    }

    #[test]
    fn test_preserve_blank_lines() {
        let input = "line1\n\nline2\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "line1\n\nline2\n");
    }

    #[test]
    fn test_preserve_indentation() {
        let input = "    indented\n\tTabbed\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "    indented\n\tTabbed\n");
    }

    #[test]
    fn test_mixed_trailing_whitespace() {
        let input = "hello  \t \nworld\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "hello\nworld\n");
    }

    // ===========================================
    // Phase 1.4: Full-width Space Detection/Fix
    // ===========================================

    #[test]
    fn test_detect_fullwidth_space() {
        let input = "hello\u{3000}world\n"; // U+3000 is full-width space
        let result = normalize_content(input);
        assert!(result.problems.iter().any(|p| p.kind == ProblemKind::FullWidthSpace));
    }

    #[test]
    fn test_fix_fullwidth_space() {
        let input = "hello\u{3000}world\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "hello world\n");
    }

    #[test]
    fn test_report_fullwidth_space_line_number() {
        let input = "line1\nline2\u{3000}here\nline3\n";
        let result = normalize_content(input);
        let problem = result.problems.iter().find(|p| p.kind == ProblemKind::FullWidthSpace);
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 2);
    }

    #[test]
    fn test_multiple_fullwidth_spaces() {
        let input = "a\u{3000}b\u{3000}c\n";
        let result = normalize_content(input);
        assert_eq!(result.content, "a b c\n");
        assert_eq!(
            result.problems.iter().filter(|p| p.kind == ProblemKind::FullWidthSpace).count(),
            2
        );
    }

    // ===========================================
    // has_changes() tests
    // ===========================================

    #[test]
    fn test_has_changes_when_content_modified() {
        let input = "hello"; // missing EOF newline
        let result = normalize_content(input);
        assert!(result.has_changes());
    }

    #[test]
    fn test_no_changes_when_content_already_normalized() {
        let input = "hello\n";
        let result = normalize_content(input);
        assert!(!result.has_changes());
    }

    #[test]
    fn test_has_changes_with_trailing_whitespace() {
        let input = "hello   \n";
        let result = normalize_content(input);
        assert!(result.has_changes());
    }
}
