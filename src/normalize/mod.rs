mod detect;
mod fix;
mod ignore;

use detect::{
    check_line_length, detect_debug_code, detect_secret_patterns, detect_todo_and_fixme_comments,
};
use fix::{
    fix_fullwidth_spaces, limit_consecutive_blank_lines, normalize_eof_newline,
    remove_code_block_remnants, remove_leading_blank_lines, remove_trailing_whitespace,
    remove_zero_width_chars,
};

pub(crate) use fix::{line_after_pre_trim_fixes, normalize_line_endings, trim_trailing_ws};

#[derive(Debug, Clone)]
pub struct NormalizeConfig {
    /// None = no limit
    pub max_blank_lines: Option<usize>,
    pub remove_zero_width: bool,
    pub remove_leading_blanks: bool,
    pub fix_code_blocks: bool,
    pub detect_todos: bool,
    pub detect_fixmes: bool,
    /// Includes patterns like console.log, print()
    pub detect_debug: bool,
    /// Also flags console.error, which is excluded by default
    pub strict_debug: bool,
    /// Detects patterns like API keys
    pub detect_secrets: bool,
    /// None = disabled
    pub max_line_length: Option<usize>,
    /// Files larger than this are skipped (like binary files) rather than
    /// fully buffered - a normalization pass generates several full-length
    /// String/Vec<&str> copies of the content, and up to CHUNK_SIZE files
    /// process in parallel, so an unbounded multi-GB text file (a log dump,
    /// generated SQL, a fixture) can OOM-kill the process (issue #103).
    pub max_file_size: u64,
}

/// Default `max_file_size`: generous for source/config files while still
/// catching the multi-GB case issue #103 is about. Override via
/// `--max-file-size` / `max_file_size` in fini.toml.
pub const DEFAULT_MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

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
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NormalizeResult {
    pub content: String,
    pub changed: bool,
    pub problems: Vec<Problem>,
    /// Problems suppressed by `fini:ignore` directives — kept (not discarded)
    /// so suppressions leave an audit trail in output and summaries.
    pub suppressed: Vec<Problem>,
    /// Some `fini:ignore` directive names only fix-type kinds, so it has no
    /// effect; callers print `FIX_KIND_IGNORE_HINT` once per run.
    pub ignores_only_fix_kinds: bool,
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

/// Identifiers of fix-type kinds, which `fini:ignore` accepts syntactically
/// but can never act on (see `ProblemKind::fix_id`, which a test keeps in
/// sync with this list).
const FIX_KIND_IDS: [&str; 5] = [
    "fullwidth",
    "leading-blanks",
    "zero-width",
    "blank-lines",
    "code-block",
];

/// Whether `id` names a fix-type kind rather than a suppressible detection.
pub(crate) fn is_fix_kind_id(id: &str) -> bool {
    FIX_KIND_IDS.contains(&id)
}

/// One-time stderr hint for a `fini:ignore` directive that names only fix
/// kinds, which silently has no effect otherwise.
pub const FIX_KIND_IGNORE_HINT: &str = "Hint: fini:ignore only suppresses detections (todo, fixme, debug, secret, line-length); fix kinds like 'fullwidth' cannot be suppressed";

/// `1..=n` over the lines of `content`: the line map before any fix has
/// dropped or moved a line.
pub(super) fn identity_line_map(content: &str) -> Vec<usize> {
    (1..=content.lines().count()).collect()
}

impl ProblemKind {
    /// Drives exit codes (`has_detection_problems`), so it has its own
    /// exhaustive match instead of deriving from `ignore_id`.
    pub fn is_detection_only(&self) -> bool {
        match self {
            ProblemKind::TodoComment
            | ProblemKind::FixmeComment
            | ProblemKind::DebugCode { .. }
            | ProblemKind::SecretPattern { .. }
            | ProblemKind::LongLine { .. } => true,
            ProblemKind::FullWidthSpace
            | ProblemKind::LeadingBlankLines { .. }
            | ProblemKind::ZeroWidthCharacter
            | ProblemKind::ExcessiveBlankLines { .. }
            | ProblemKind::CodeBlockRemnant => false,
        }
    }

    /// Directive-syntax identifier of a fix kind; `None` for detections.
    /// Exhaustive so a new variant forces a decision here.
    pub fn fix_id(&self) -> Option<&'static str> {
        match self {
            ProblemKind::FullWidthSpace => Some("fullwidth"),
            ProblemKind::LeadingBlankLines { .. } => Some("leading-blanks"),
            ProblemKind::ZeroWidthCharacter => Some("zero-width"),
            ProblemKind::ExcessiveBlankLines { .. } => Some("blank-lines"),
            ProblemKind::CodeBlockRemnant => Some("code-block"),
            ProblemKind::TodoComment
            | ProblemKind::FixmeComment
            | ProblemKind::DebugCode { .. }
            | ProblemKind::SecretPattern { .. }
            | ProblemKind::LongLine { .. } => None,
        }
    }

    /// Identifier used in `fini:ignore` directives; `None` for fix kinds,
    /// which a directive can never suppress. Exhaustive so a new variant
    /// forces a decision here.
    pub fn ignore_id(&self) -> Option<&'static str> {
        match self {
            ProblemKind::TodoComment => Some("todo"),
            ProblemKind::FixmeComment => Some("fixme"),
            ProblemKind::DebugCode { .. } => Some("debug"),
            ProblemKind::SecretPattern { .. } => Some("secret"),
            ProblemKind::LongLine { .. } => Some("line-length"),
            ProblemKind::FullWidthSpace
            | ProblemKind::LeadingBlankLines { .. }
            | ProblemKind::ZeroWidthCharacter
            | ProblemKind::ExcessiveBlankLines { .. }
            | ProblemKind::CodeBlockRemnant => None,
        }
    }
}

pub fn normalize_content(content: &str, config: &NormalizeConfig) -> NormalizeResult {
    let mut result = content.to_string();
    let mut problems = vec![];

    result = normalize_line_endings(&result);

    // Maps each line of `result` (0-indexed) to its original file line number
    // (1-indexed). Fixes that drop or reorder lines update this map so every
    // problem — from fixes and from detections that run later on the fully
    // normalized content — reports the line the user actually has open,
    // rather than a post-fix line number (issue #76).
    let mut line_map: Vec<usize> = identity_line_map(&result);

    if config.remove_zero_width {
        debug_assert!(line_map.len() >= result.lines().count());
        let (fixed, zw_problems) = remove_zero_width_chars(&result, &line_map);
        result = fixed;
        problems.extend(zw_problems);
    }

    if config.fix_code_blocks {
        debug_assert!(line_map.len() >= result.lines().count());
        let (fixed, new_line_map, code_block_problems) =
            remove_code_block_remnants(&result, &line_map);
        result = fixed;
        line_map = new_line_map;
        problems.extend(code_block_problems);
    }

    if config.remove_leading_blanks {
        debug_assert!(line_map.len() >= result.lines().count());
        let (fixed, new_line_map, leading_problems) =
            remove_leading_blank_lines(&result, &line_map);
        result = fixed;
        line_map = new_line_map;
        problems.extend(leading_problems);
    }

    if let Some(max) = config.max_blank_lines {
        debug_assert!(line_map.len() >= result.lines().count());
        let (fixed, new_line_map, blank_problems) =
            limit_consecutive_blank_lines(&result, max, &line_map);
        result = fixed;
        line_map = new_line_map;
        problems.extend(blank_problems);
    }

    debug_assert!(line_map.len() >= result.lines().count());
    let (fixed, fullwidth_problems) = fix_fullwidth_spaces(&result, &line_map);
    result = fixed;
    problems.extend(fullwidth_problems);

    // `line_map` is no longer updated past this point: none of the remaining
    // fixes may remove or reorder a non-trailing line without threading a new
    // map through, the same way the fixes above do. Trailing-newline
    // collapsing below is fine as-is since it only shortens the tail.
    result = remove_trailing_whitespace(&result);
    result = normalize_eof_newline(&result);

    if config.detect_todos || config.detect_fixmes {
        let (todo_problems, fixme_problems) = detect_todo_and_fixme_comments(&result, &line_map);
        if config.detect_todos {
            problems.extend(todo_problems);
        }
        if config.detect_fixmes {
            problems.extend(fixme_problems);
        }
    }

    if config.detect_debug {
        problems.extend(detect_debug_code(&result, config.strict_debug, &line_map));
    }

    if config.detect_secrets {
        problems.extend(detect_secret_patterns(&result, &line_map));
    }

    if let Some(max_length) = config.max_line_length {
        problems.extend(check_line_length(&result, max_length, &line_map));
    }

    // Partition (not filter) so suppressed problems stay auditable (issue #46)
    let mut suppressed = vec![];
    let mut ignores_only_fix_kinds = false;
    if !problems.is_empty() {
        let ignore_map = ignore::parse_ignore_directives(&result, &line_map);
        ignores_only_fix_kinds = ignore_map.has_fix_kinds_only_directive();
        if !ignore_map.is_empty() {
            let (sup, kept): (Vec<_>, Vec<_>) = problems
                .into_iter()
                .partition(|p| ignore_map.is_ignored(p.line, &p.kind));
            suppressed = sup;
            problems = kept;
        }
    }

    let changed = result != content;

    NormalizeResult {
        content: result,
        changed,
        problems,
        suppressed,
        ignores_only_fix_kinds,
    }
}

/// Replace secret-matching lines with hint-only placeholders (see detect.rs).
pub fn mask_secret_lines(content: &str) -> String {
    detect::mask_secret_lines(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant, walked through a no-wildcard match: adding a variant
    /// fails to compile in `next` instead of silently escaping the tests
    /// below (a fixed-length array would not).
    fn all_kinds() -> Vec<ProblemKind> {
        // Invariant: exactly one arm returns `None` (the terminal). A new
        // variant must be chained from the current terminal and become the
        // new terminal; a bare `NewKind => None` arm would drop it from the
        // walk. The assert below pins the terminal to the last-declared variant.
        fn next(kind: &ProblemKind) -> Option<ProblemKind> {
            match kind {
                ProblemKind::FullWidthSpace => Some(ProblemKind::LeadingBlankLines { count: 1 }),
                ProblemKind::LeadingBlankLines { .. } => Some(ProblemKind::ZeroWidthCharacter),
                ProblemKind::ZeroWidthCharacter => {
                    Some(ProblemKind::ExcessiveBlankLines { found: 3, limit: 2 })
                }
                ProblemKind::ExcessiveBlankLines { .. } => Some(ProblemKind::CodeBlockRemnant),
                ProblemKind::CodeBlockRemnant => Some(ProblemKind::TodoComment),
                ProblemKind::TodoComment => Some(ProblemKind::FixmeComment),
                ProblemKind::FixmeComment => Some(ProblemKind::DebugCode { pattern: "print(" }),
                ProblemKind::DebugCode { .. } => Some(ProblemKind::SecretPattern { hint: "key" }),
                ProblemKind::SecretPattern { .. } => Some(ProblemKind::LongLine {
                    length: 100,
                    limit: 80,
                }),
                ProblemKind::LongLine { .. } => None,
            }
        }
        let mut kinds = vec![ProblemKind::FullWidthSpace];
        while let Some(k) = next(kinds.last().unwrap()) {
            kinds.push(k);
        }
        // Keep in sync with the last variant declared in `ProblemKind`.
        assert!(
            matches!(kinds.last(), Some(ProblemKind::LongLine { .. })),
            "walk must end at the last-declared ProblemKind variant"
        );
        kinds
    }

    #[test]
    fn test_fix_kind_ids_match_ignore_id_split() {
        let kinds = all_kinds();
        let fix_ids: Vec<&str> = kinds.iter().filter_map(ProblemKind::fix_id).collect();
        assert_eq!(FIX_KIND_IDS.to_vec(), fix_ids);
        for kind in &kinds {
            // Each kind is exactly one of fix or detection
            assert_ne!(kind.fix_id().is_some(), kind.ignore_id().is_some());
            assert_eq!(kind.is_detection_only(), kind.ignore_id().is_some());
            if let Some(id) = kind.ignore_id() {
                assert!(!is_fix_kind_id(id), "{id}");
                assert!(FIX_KIND_IGNORE_HINT.contains(id), "hint omits {id}");
            }
        }
    }

    #[test]
    fn test_fix_kind_hint_says_cannot_be_suppressed() {
        assert!(FIX_KIND_IGNORE_HINT.contains("cannot be suppressed"));
        assert!(!FIX_KIND_IGNORE_HINT.contains("always applied"));
    }

    #[test]
    fn test_first_directive_on_line_decides() {
        // A later `fini:ignore-next-line` on the same line is just text after
        // the first directive (see README), so line 2 stays reported.
        for input in [
            "// TODO x fini:ignore fini:ignore-next-line\n// TODO y\n",
            "// see fini:ignored; fini:ignore-next-line\n// TODO y\n",
        ] {
            let result = normalize_content(input, &NormalizeConfig::default());
            assert!(
                result
                    .problems
                    .iter()
                    .any(|p| p.kind == ProblemKind::TodoComment && p.line == 2),
                "{input:?}"
            );
        }
    }

    #[test]
    fn test_ignore_next_line_does_not_suppress_fix_kind() {
        for input in [
            "# fini:ignore-next-line\na\u{3000}b\n",
            "# fini:ignore-next-line fullwidth\na\u{3000}b\n",
        ] {
            let result = normalize_content(input, &NormalizeConfig::default());
            assert!(result.suppressed.is_empty(), "{input:?}");
            assert!(
                result
                    .problems
                    .iter()
                    .any(|p| p.kind == ProblemKind::FullWidthSpace && p.line == 2),
                "{input:?}"
            );
        }
    }

    #[test]
    fn test_ignore_does_not_suppress_zero_width() {
        let result = normalize_content("x\u{200B}y # fini:ignore\n", &NormalizeConfig::default());
        assert!(result.suppressed.is_empty());
        assert!(result
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::ZeroWidthCharacter && p.line == 1));
    }

    #[test]
    fn test_fix_kinds_only_directive_sets_flag() {
        let config = NormalizeConfig::default();
        let flagged = |input: &str| normalize_content(input, &config).ignores_only_fix_kinds;
        assert!(flagged("a\u{3000}b # fini:ignore fullwidth\n"));
        assert!(!flagged("a\u{3000}b # fini:ignore\n"));
        assert!(!flagged("a\u{3000}b # fini:ignore todo\n"));
    }

    #[test]
    fn test_ignored_problems_recorded_as_suppressed() {
        let input = "password = \"supersecret123\" # fini:ignore secret\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result
            .problems
            .iter()
            .all(|p| !matches!(p.kind, ProblemKind::SecretPattern { .. })));
        assert_eq!(result.suppressed.len(), 1);
        assert!(matches!(
            result.suppressed[0].kind,
            ProblemKind::SecretPattern { .. }
        ));
    }

    #[test]
    fn test_ignore_directive_suppressed_line_uses_original_number_after_shift() {
        let input = "\n\npassword = \"supersecret123\" # fini:ignore secret\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.suppressed.len(), 1);
        assert_eq!(result.suppressed[0].line, 3);
    }

    #[test]
    fn test_ignore_next_line_targets_literal_next_line_after_shift() {
        let input = "\n\n// fini:ignore-next-line\n// TODO: x\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result
            .problems
            .iter()
            .all(|p| !matches!(p.kind, ProblemKind::TodoComment)));
        assert_eq!(result.suppressed.len(), 1);
        assert_eq!(result.suppressed[0].line, 4);
    }

    #[test]
    fn test_no_directives_leaves_suppressed_empty() {
        let input = "// TODO: fix this\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert!(result.suppressed.is_empty());
        assert_eq!(result.problems.len(), 1);
    }

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

    #[test]
    fn test_whitespace_only_normalizes_in_one_pass_when_keeping_leading_blanks() {
        let config = NormalizeConfig {
            remove_leading_blanks: false,
            ..NormalizeConfig::default()
        };
        for input in ["\n \n", "\r\r", "\n\n\n"] {
            let once = normalize_content(input, &config).content;
            assert_eq!(once, "", "input {input:?}");
            assert_eq!(normalize_content(&once, &config).content, once);
        }
    }

    #[test]
    fn test_bom_does_not_block_leading_blank_removal() {
        let input = "\u{FEFF}\n\nfoo\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        assert_eq!(result.content, "\u{FEFF}foo\n");
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::LeadingBlankLines { count: 2 }));
        assert_eq!(problem.map(|p| p.line), Some(1));
        let again = normalize_content(&result.content, &NormalizeConfig::default());
        assert_eq!(again.content, result.content);
    }

    #[test]
    fn test_bom_only_and_blank_inputs_do_not_panic_and_are_idempotent() {
        for fix_code_blocks in [false, true] {
            for remove_leading_blanks in [true, false] {
                let config = NormalizeConfig {
                    fix_code_blocks,
                    remove_leading_blanks,
                    ..NormalizeConfig::default()
                };
                let mut inputs = vec![
                    "\u{FEFF}",
                    "\u{FEFF}\n",
                    "\u{FEFF}\n\n",
                    "\u{FEFF} \n",
                    "\u{FEFF}\r\n",
                ];
                if fix_code_blocks {
                    inputs.extend(["\u{FEFF}```\n", "\u{FEFF}```py\n```\n"]);
                }
                for input in inputs {
                    let first = normalize_content(input, &config);
                    assert_eq!(first.content, "\u{FEFF}\n", "input {input:?}");
                    let second = normalize_content(&first.content, &config);
                    assert_eq!(second.content, first.content, "input {input:?}");
                    assert!(second.problems.is_empty(), "input {input:?}");
                }
            }
        }
    }

    #[test]
    fn test_bom_does_not_block_first_line_fence_removal() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        let input = "\u{FEFF}```py\nx\n```\n";
        let result = normalize_content(input, &config);
        assert_eq!(result.content, "\u{FEFF}x\n");
        assert!(result
            .problems
            .iter()
            .any(|p| matches!(p.kind, ProblemKind::CodeBlockRemnant) && p.line == 1));
        let again = normalize_content(&result.content, &config);
        assert_eq!(again.content, result.content);
    }

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
    fn test_code_block_fence_padding_is_space_tab_fullwidth_only() {
        let config = NormalizeConfig {
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        for kept in ["\u{0c}```\n", "```\u{a0}\n"] {
            let result = normalize_content(kept, &config);
            assert_eq!(result.content, kept);
            assert!(result.problems.is_empty());
            assert_eq!(normalize_content(&result.content, &config).content, kept);
        }
        for fence in [
            "```rust\n",
            "  ```  \n",
            "\t```\t\n",
            "```\u{3000}\n",
            "\u{3000}```\n",
            "```rust\u{3000}\n",
        ] {
            let result = normalize_content(fence, &config);
            assert_eq!(result.content, "");
            assert!(result
                .problems
                .iter()
                .any(|p| matches!(p.kind, ProblemKind::CodeBlockRemnant)));
            let wrapped = format!("a\n{fence}b\n");
            let once = normalize_content(&wrapped, &config).content;
            assert_eq!(once, "a\nb\n");
            assert_eq!(normalize_content(&once, &config).content, once);
        }
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
    fn test_form_feed_and_nbsp_lines_are_not_blank() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            ..NormalizeConfig::default()
        };
        for ch in ['\u{0c}', '\u{a0}', '\u{2028}', '\u{85}'] {
            let leading = format!("{ch}\nfoo\n");
            assert_eq!(normalize_content(&leading, &config).content, leading);
            // Same input behind a BOM exercises the BOM branch of the leading-blank fix.
            let bom_leading = format!("\u{feff}{leading}");
            assert_eq!(
                normalize_content(&bom_leading, &config).content,
                bom_leading
            );
            let run = format!("a\n{ch}\n{ch}\n{ch}\nb\n");
            let result = normalize_content(&run, &config);
            assert_eq!(result.content, run);
            assert!(result.problems.is_empty());
        }
    }

    #[test]
    fn test_fullwidth_only_lines_still_blank_and_idempotent() {
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            ..NormalizeConfig::default()
        };
        let input = "\u{3000}\n\u{3000}\na\n\u{3000}\n\u{3000}\n\u{3000}\nb\n";
        let once = normalize_content(input, &config).content;
        assert_eq!(once, "a\n\nb\n");
        assert_eq!(normalize_content(&once, &config).content, once);
    }

    #[test]
    fn test_fix_fullwidth_spaces_short_line_map_without_fullwidth_does_not_panic() {
        let (result, problems) = fix::fix_fullwidth_spaces("a\nb\nc\n", &[]);
        assert_eq!(result, "a\nb\nc\n");
        assert!(problems.is_empty());
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
    fn test_fix_output_is_idempotent_with_blank_limit_and_code_blocks() {
        // Regression test for issue #33: max_blank_lines + fix_code_blocks must be
        // idempotent — running normalize_content twice should reach a fixed point
        // on the first pass, so `fini` followed by `fini --check` never fails.
        let config = NormalizeConfig {
            max_blank_lines: Some(1),
            fix_code_blocks: true,
            ..NormalizeConfig::default()
        };
        let input = "a\n\n```py\n\nb\n";
        let first = normalize_content(input, &config);
        assert_eq!(first.content, "a\n\nb\n");

        let second = normalize_content(&first.content, &config);
        assert!(!second.has_changes());
        assert!(second.problems.is_empty());
    }

    #[test]
    fn test_idempotency_matrix_across_fix_flag_combinations() {
        // Broader regression coverage for issue #33's bug class: any combination
        // of blank-line/code-block fix flags applied to adversarial inputs (a
        // fence at the very start/end of the file, fences surrounded by blank
        // lines, CRLF mixed with trailing whitespace around a fence) must reach
        // a fixed point after a single normalize_content pass.
        let inputs = [
            "```rust\nfn main() {}\n```\n",
            "\n\n```py\ncode\n```\n\nafter\n",
            "before\n```py\n\ncode\n\n```\nafter\n",
            "```\ncode\n```\n\n\n\nmore\n",
            "line1\r\n\r\n```js\r\ncode   \r\n```\r\n\r\nline2\r\n",
            "\n\n\n```\ncode\n```\n",
        ];

        for &max_blank_lines in &[None, Some(0), Some(1), Some(2)] {
            for &fix_code_blocks in &[false, true] {
                for &remove_leading_blanks in &[false, true] {
                    let config = NormalizeConfig {
                        max_blank_lines,
                        fix_code_blocks,
                        remove_leading_blanks,
                        ..NormalizeConfig::default()
                    };

                    for input in &inputs {
                        let first = normalize_content(input, &config);
                        let second = normalize_content(&first.content, &config);
                        assert!(
                            !second.has_changes(),
                            "not idempotent for input {input:?} with config {config:?}: \
                             {:?} -> {:?}",
                            first.content,
                            second.content
                        );
                        assert!(
                            second.problems.iter().all(|p| p.kind.is_detection_only()),
                            "leftover fixable problems for input {input:?} with config \
                             {config:?}: {:?}",
                            second.problems
                        );
                    }
                }
            }
        }
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
    fn test_unquoted_env_var_reference_not_detected() {
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

    #[test]
    fn test_ignore_does_not_suppress_fix_kinds() {
        for input in [
            "a\u{3000}b # fini:ignore fullwidth\n",
            "a\u{3000}b # fini:ignore\n",
        ] {
            let result = normalize_content(input, &NormalizeConfig::default());
            assert!(
                result
                    .problems
                    .iter()
                    .any(|p| p.kind == ProblemKind::FullWidthSpace && p.line == 1),
                "{input:?}"
            );
            assert!(result.suppressed.is_empty());
        }
    }

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

    #[test]
    fn test_detection_line_number_reports_original_line_after_leading_blanks_removed() {
        // Regression test for issue #76: detections run on the post-fix
        // content, but --check never writes, so the reported line must match
        // the original file the user has open, not the shifted content.
        let input = "\n\n\n// TODO: fix this\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::TodoComment));
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 4);
    }

    #[test]
    fn test_fullwidth_space_line_number_reports_original_line_after_leading_blanks_removed() {
        // Same regression as the TODO case above, but for a problem that
        // itself comes out of a fix pass (not a detector) — fix_fullwidth_spaces
        // runs after leading-blank removal, so it must consult the shifted
        // line_map too, not just the final detectors (issue #76).
        let input = "\n\nhello\u{3000}world\n";
        let result = normalize_content(input, &NormalizeConfig::default());
        let problem = result
            .problems
            .iter()
            .find(|p| p.kind == ProblemKind::FullWidthSpace);
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 3);
    }

    #[test]
    fn test_detection_line_number_reports_original_line_after_code_block_and_blank_shift() {
        // Combines two line-shifting fixes (code block remnant removal, then
        // blank-line limiting) ahead of a detection, to make sure line-map
        // updates compose correctly across fix stages (issue #76).
        let config = NormalizeConfig {
            fix_code_blocks: true,
            max_blank_lines: Some(0),
            ..NormalizeConfig::default()
        };
        // original lines: 1 ```rust  2 fn main() {}  3 (blank)  4 (blank)  5 // TODO: fix
        let input = "```rust\nfn main() {}\n\n\n// TODO: fix\n";
        let result = normalize_content(input, &config);
        let problem = result
            .problems
            .iter()
            .find(|p| matches!(p.kind, ProblemKind::TodoComment));
        assert!(problem.is_some());
        assert_eq!(problem.unwrap().line, 5);
    }
}
