use crate::colors::Colors;
use crate::normalize::{
    line_after_pre_trim_fixes, mask_secret_lines, normalize_line_endings, trim_trailing_ws,
    NormalizeConfig, NormalizeResult, Problem, ProblemKind,
};
use similar::{ChangeTag, DiffTag, TextDiff};
use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;

/// Escapes `\r` and `\n` (and ESC / C1 controls, via `escape_terminal_controls`)
/// so `s` is safe to print as a standalone output line. A string containing
/// line breaks would otherwise forge extra output
/// lines (issue #87) - e.g. `--quiet` mode's one-path-per-line contract,
/// which scripts parse, or a fake "Fixed: <other-file>" line spoofing a
/// result for a file that was never touched. Used both for a path's own
/// `Display` form (`safe_path_display`) and for error text that embeds a
/// path (e.g. a walk error from the `ignore` crate), since by the time
/// that text reaches us it's already a flattened string, not a `Path`.
pub fn escape_line_breaks(s: &str) -> String {
    escape_terminal_controls(s)
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

/// A path's `Display` form, safe to print as a standalone output line.
/// See `escape_line_breaks`.
pub fn safe_path_display(path: &Path) -> String {
    escape_line_breaks(&path.display().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Normal,
    Quiet,
    Diff,
}

pub struct Config {
    pub check_only: bool,
    pub output_mode: OutputMode,
    /// Whether `--diff` was passed on the CLI. Tracked separately from
    /// `output_mode` because `--diff --quiet` together resolve to
    /// `OutputMode::Quiet` (quiet display wins), and `should_write` must
    /// still honor `--diff`'s no-write guarantee in that combination -
    /// deriving "is this a diff preview" from `output_mode == Diff` alone
    /// let `--quiet` silently re-enable writes (issue #89).
    pub diff: bool,
    pub normalize: NormalizeConfig,
    pub exclude_patterns: Vec<String>,
    /// Whether `--hidden` was passed: include dotfiles (.env,
    /// .github/workflows/*.yml) in directory scans instead of always
    /// skipping them, since that's where secrets land most often
    /// (issue #101). `.git/` is still excluded either way.
    pub include_hidden: bool,
}

impl Config {
    /// Whether normalized content should actually be written to disk.
    /// Check mode never writes (`check_only` also gates the exit-code check
    /// in main.rs and keeps that meaning); `--diff` alone is a preview per
    /// the README ("Preview changes") and must not write either.
    #[must_use]
    pub fn should_write(&self) -> bool {
        !self.check_only && !self.diff
    }
}

pub struct OutputContext {
    pub(crate) mode: OutputMode,
    pub(crate) colors: Colors,
    pub(crate) verbose: bool,
    pub(crate) show_progress: bool,
}

impl OutputContext {
    pub fn new(mode: OutputMode, use_colors: bool, verbose: bool, show_progress: bool) -> Self {
        Self {
            mode,
            colors: Colors::new(use_colors),
            verbose: verbose && mode != OutputMode::Quiet,
            show_progress,
        }
    }
}

pub struct RunResult {
    pub files_fixed: usize,
    pub files_with_problems: usize,
    pub warnings: usize,
    pub errors: usize,
    /// Files not inspected (binary, non-UTF-8, UTF-16, symlink) — surfaced in
    /// the summary so skipped coverage is visible without --verbose (issue #40)
    pub skipped: usize,
    /// Problems suppressed via fini:ignore directives (issue #46)
    pub suppressed: usize,
    pub suppressed_secrets: usize,
}

impl RunResult {
    pub fn has_problems(&self) -> bool {
        self.files_with_problems > 0
    }

    pub fn has_errors(&self) -> bool {
        self.errors > 0
    }
}

pub fn print_check_result(
    path: &Path,
    original: &str,
    result: &NormalizeResult,
    ctx: &OutputContext,
    remove_zero_width: bool,
) {
    if ctx.mode == OutputMode::Quiet {
        println!("{}", safe_path_display(path));
        return;
    }

    let stdout = io::stdout();
    // Panicking on a failed stdout write matches println!'s historical behavior
    print_check_report_to(
        &mut stdout.lock(),
        &safe_path_display(path),
        original,
        result,
        ctx.mode == OutputMode::Diff,
        &ctx.colors,
        remove_zero_width,
    )
    .expect("failed to write to stdout");
}

/// Writes one input's check-mode report: the masked diff or an
/// `Error: <label>` header, then the problem list. Shared by file mode
/// (stdout) and `--stdin --check` (stderr). `label` must already be safe to
/// print as a line (see `safe_path_display`).
pub fn print_check_report_to<W: Write>(
    w: &mut W,
    label: &str,
    original: &str,
    result: &NormalizeResult,
    show_diff: bool,
    colors: &Colors,
    remove_zero_width: bool,
) -> io::Result<()> {
    // Mirrors print_fix_result's Diff branch: masked the same way (issue
    // #44's contract applies to every diff path, not just fix mode). Every
    // path must attach a label to the problem list below (issue #104):
    // print the masked diff when masking doesn't collapse the change to
    // identical text, otherwise an `Error:` header - callers only reach this
    // when has_changes() or the problem list is non-empty.
    let masked_diff = show_diff
        .then(|| masked_pair(original, &result.content))
        .filter(|(orig, new)| orig != new);

    if let Some((orig, new)) = masked_diff {
        print_diff_to(w, label, &orig, &new)?;
        if masked_diff_hides_changes(original, &result.content) {
            print_change_summary_to(w, original, &result.content, remove_zero_width)?;
        }
    } else {
        writeln!(w, "{}Error:{} {}", colors.error, colors.reset(), label)?;

        if result.has_changes() {
            print_change_summary_to(w, original, &result.content, remove_zero_width)?;
        }
    }

    print_problems_to(w, &result.problems)
}

/// Prints the content-diff-derived summary lines (line endings, missing EOF
/// newline, extra trailing newlines, trailing whitespace). Shown in place of
/// a diff for Normal-mode check output and `--stdin --check` without
/// `--diff`, under the `Would fix:` / `Error:` header when masking collapses
/// a `--diff` (fix mode, check mode, `--stdin --check --diff`), and after a
/// partly masked diff for changes on masked lines. Shared by file mode (to
/// stdout) and `--stdin --check` (to stderr, since stdin's stdout is
/// reserved for normalized content only - issue #38); fix-only problems
/// never populate `result.problems`, so these bullets are the only report
/// of them (issue #79).
pub fn print_change_summary_to<W: Write>(
    w: &mut W,
    original: &str,
    result_content: &str,
    remove_zero_width: bool,
) -> io::Result<()> {
    // A lone `\r` is itself a line ending that normalization collapses to
    // `\n` (see normalize::fix::normalize_line_endings), but str::lines()
    // doesn't split on it and trim_end_matches(['\n', '\r']) eats it as
    // trailing "whitespace" — so counting newlines and walking lines against
    // the raw `original` misattributes a CR-only file's EOF/trailing-
    // whitespace bullets. Normalize line endings first so those bullets only
    // fire for a change distinct from the line-ending one reported below
    // (issue #83).
    let original_lf = normalize_line_endings(original);

    let orig_trailing_newlines = original_lf.len() - original_lf.trim_end_matches('\n').len();
    let result_trailing_newlines =
        result_content.len() - result_content.trim_end_matches('\n').len();

    if orig_trailing_newlines == 0 && result_trailing_newlines > 0 {
        writeln!(w, "  - missing EOF newline")?;
    } else if orig_trailing_newlines > 1 && result_trailing_newlines < orig_trailing_newlines {
        writeln!(w, "  - extra trailing newline(s) removed")?;
    }

    if original.contains('\r') {
        writeln!(w, "  - CRLF/CR line endings normalized to LF")?;
    }

    for (i, orig_line) in original_lf.lines().enumerate() {
        // See line_after_pre_trim_fixes's doc for why the trim check runs
        // against the pre-trim-fixed line rather than the raw one
        // (issue #94).
        let after_earlier_fixes = line_after_pre_trim_fixes(orig_line, remove_zero_width);
        if after_earlier_fixes.len() != trim_trailing_ws(&after_earlier_fixes).len() {
            writeln!(w, "  - trailing whitespace at line {}", i + 1)?;
        }
    }
    Ok(())
}

/// Prints the per-problem diagnostic list (e.g. "- TODO comment at line 3").
/// Shared by file-mode check output and `--stdin --check` (issue #38, #79).
pub fn print_problems_to<W: Write>(w: &mut W, problems: &[Problem]) -> io::Result<()> {
    for problem in problems {
        match &problem.kind {
            ProblemKind::FullWidthSpace => {
                writeln!(w, "  - full-width space at line {}", problem.line)?;
            }
            ProblemKind::LeadingBlankLines { count } => {
                writeln!(w, "  - {} leading blank line(s)", count)?;
            }
            ProblemKind::ZeroWidthCharacter => {
                writeln!(w, "  - zero-width character at line {}", problem.line)?;
            }
            ProblemKind::ExcessiveBlankLines { found, limit } => {
                writeln!(
                    w,
                    "  - {} consecutive blank lines at line {} (limit: {})",
                    found, problem.line, limit
                )?;
            }
            ProblemKind::CodeBlockRemnant => {
                writeln!(w, "  - code block remnant at line {}", problem.line)?;
            }
            ProblemKind::TodoComment => {
                writeln!(w, "  - TODO comment at line {}", problem.line)?;
            }
            ProblemKind::FixmeComment => {
                writeln!(w, "  - FIXME comment at line {}", problem.line)?;
            }
            ProblemKind::DebugCode { pattern } => {
                writeln!(w, "  - debug code '{}' at line {}", pattern, problem.line)?;
            }
            ProblemKind::SecretPattern { hint } => {
                writeln!(
                    w,
                    "  - potential secret ({}) at line {}",
                    hint, problem.line
                )?;
            }
            ProblemKind::LongLine { length, limit } => {
                writeln!(
                    w,
                    "  - line {} is too long ({} > {} chars)",
                    problem.line, length, limit
                )?;
            }
        }
    }
    Ok(())
}

pub fn print_fix_result(
    path: &Path,
    original: &str,
    result: &NormalizeResult,
    ctx: &OutputContext,
    remove_zero_width: bool,
) {
    match ctx.mode {
        OutputMode::Quiet => println!("{}", safe_path_display(path)),
        OutputMode::Diff => {
            let (orig, new) = masked_pair(original, &result.content);
            if orig != new {
                print_diff(&safe_path_display(path), &orig, &new);
                if masked_diff_hides_changes(original, &result.content) {
                    print_change_summary_to(
                        &mut io::stdout().lock(),
                        original,
                        &result.content,
                        remove_zero_width,
                    )
                    .expect("failed to write to stdout");
                }
            } else {
                // No diff header to attach the problem list to (issue #104).
                // Always named: the file may be counted as fixed with no
                // problems at all (issue #185).
                let (color, label) = if result.has_changes() {
                    (ctx.colors.success, "Would fix:")
                } else {
                    (ctx.colors.warning, "Detected:")
                };
                println!(
                    "{}{}{} {}",
                    color,
                    label,
                    ctx.colors.reset(),
                    safe_path_display(path)
                );
                // Line numbers and category names only, so safe under masking
                // (issue #193).
                if result.has_changes() {
                    print_change_summary_to(
                        &mut io::stdout().lock(),
                        original,
                        &result.content,
                        remove_zero_width,
                    )
                    .expect("failed to write to stdout");
                }
            }
            let stdout = io::stdout();
            print_problems_to(&mut stdout.lock(), &result.problems)
                .expect("failed to write to stdout");
        }
        OutputMode::Normal => {
            for problem in result
                .problems
                .iter()
                .filter(|p| matches!(p.kind, ProblemKind::FullWidthSpace))
            {
                println!(
                    "{}Warning:{} {}:{} full-width space",
                    ctx.colors.warning,
                    ctx.colors.reset(),
                    safe_path_display(path),
                    problem.line
                );
            }

            if result.has_changes() {
                println!(
                    "{}Fixed:{} {}",
                    ctx.colors.success,
                    ctx.colors.reset(),
                    safe_path_display(path)
                );
            } else {
                // Nothing was rewritten, so the file only has detection-only
                // problems (TODOs, debug code, secrets) — label it distinctly
                // from a rewritten file.
                println!(
                    "{}Detected:{} {}",
                    ctx.colors.warning,
                    ctx.colors.reset(),
                    safe_path_display(path)
                );
            }

            // Detection-only problems (TODOs, debug code, secrets) never
            // change result.content, so a fix that also rewrote the file
            // (has_changes() true) must still surface them here — fix mode
            // never fails on them (see README), so this list is the only way
            // they're reported (issue #78).
            let detections: Vec<Problem> = result
                .problems
                .iter()
                .filter(|p| p.kind.is_detection_only())
                .cloned()
                .collect();
            if !detections.is_empty() {
                let stdout = io::stdout();
                print_problems_to(&mut stdout.lock(), &detections)
                    .expect("failed to write to stdout");
            }
        }
    }
}

/// Leaves an stderr audit trail for each secret detection suppressed by
/// `fini:ignore`, and returns how many there were. `label` must already be
/// safe to print as a line (see `safe_path_display`).
pub fn warn_suppressed_secrets(label: &str, suppressed: &[Problem]) -> usize {
    let mut count = 0;
    for problem in suppressed {
        if let ProblemKind::SecretPattern { hint } = &problem.kind {
            count += 1;
            eprintln!(
                "Warning: {}:{} potential secret ({hint}) suppressed by fini:ignore",
                label, problem.line
            );
        }
    }
    count
}

pub fn print_checked(path: &Path, ctx: &OutputContext) {
    println!(
        "{}Checked:{} {}",
        ctx.colors.info,
        ctx.colors.reset(),
        safe_path_display(path)
    );
}

pub fn print_skipped(path: &Path, reason: &str, ctx: &OutputContext) {
    println!(
        "{}Skipping {}: {}{}",
        ctx.colors.info,
        reason,
        ctx.colors.reset(),
        safe_path_display(path)
    );
}

/// Mask secret-matching lines on both sides of a diff before printing, so the
/// diff path honors the same hint-only contract as check output (issue #44).
/// Unconditional: masking is an output guarantee independent of whether
/// secret *detection* is enabled (issue #93), so there's no caller-supplied
/// toggle here to accidentally disable it.
fn masked_pair(original: &str, content: &str) -> (String, String) {
    (mask_secret_lines(original), mask_secret_lines(content))
}

pub fn print_diff(label: &str, original: &str, content: &str) {
    let mut stdout = io::stdout().lock();
    // Panicking on a failed stdout write matches println!'s historical behavior
    print_diff_to(&mut stdout, label, original, content).expect("failed to write diff to stdout");
}

pub fn print_diff_to<W: Write>(
    w: &mut W,
    label: &str,
    original: &str,
    content: &str,
) -> io::Result<()> {
    let diff = TextDiff::from_lines(original, content);

    writeln!(w, "--- {label}")?;
    writeln!(w, "+++ {label}")?;

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            writeln!(w)?;
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                write!(w, "{sign}{}", escape_terminal_controls(&change.to_string()))?;
            }
        }
    }
    Ok(())
}

/// Escapes ESC and C1 controls (incl. NEL U+0085) in diff text, so a file
/// cannot smuggle terminal escape sequences that hide or rewrite output.
/// Other C0 controls pass through: they have no terminal-command meaning
/// that reaches past the current line, and `\t` / the line-ending `\n` must
/// stay intact.
fn escape_terminal_controls(s: &str) -> Cow<'_, str> {
    let needs_escape = |c: char| c == '\u{1B}' || ('\u{80}'..='\u{9F}').contains(&c);
    if !s.contains(needs_escape) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{1B}' => out.push_str("\\x1b"),
            c if needs_escape(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Whether the masked diff hides a real change. It is computed on the masked
/// lines (as displayed), so a change is hidden when an `Equal` op covers raw
/// lines that differ, or when a changed op touches a masked line (its raw
/// content is not shown). Masking preserves the `\n`-line count, so raw and
/// masked lines share indices.
fn masked_diff_hides_changes(original: &str, content: &str) -> bool {
    let raw_old: Vec<&str> = original.split_inclusive('\n').collect();
    let raw_new: Vec<&str> = content.split_inclusive('\n').collect();
    let (masked_old_text, masked_new_text) = masked_pair(original, content);
    let masked_old: Vec<&str> = masked_old_text.split_inclusive('\n').collect();
    let masked_new: Vec<&str> = masked_new_text.split_inclusive('\n').collect();
    if raw_old.len() != masked_old.len() || raw_new.len() != masked_new.len() {
        return true;
    }

    // Trim `\n` only: a `\r\n` vs `\n` difference is a real change.
    let line = |s: &str| s.trim_end_matches('\n').to_owned();
    let diff = TextDiff::configure().diff_slices(&masked_old, &masked_new);
    diff.ops().iter().any(|op| {
        let (tag, mut old, mut new) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            old.zip(new)
                .any(|(i, j)| line(raw_old[i]) != line(raw_new[j]))
        } else {
            old.any(|i| raw_old[i] != masked_old[i]) || new.any(|j| raw_new[j] != masked_new[j])
        }
    })
}

pub fn print_summary(result: &RunResult, config: &Config, ctx: &OutputContext) {
    if ctx.mode == OutputMode::Quiet {
        return;
    }

    let colored = |color: &str, text: String| format!("{color}{text}{}", ctx.colors.reset());
    let mut parts = vec![];

    if config.check_only {
        if result.files_with_problems > 0 {
            parts.push(colored(
                ctx.colors.error,
                format!("{} files with problems", result.files_with_problems),
            ));
        }
    } else {
        if result.files_fixed > 0 {
            // Bare --diff previews changes without writing them (README:
            // "Preview changes"), so the count is what *would* be fixed.
            let label = if config.should_write() {
                "files fixed"
            } else {
                "files would be fixed"
            };
            parts.push(colored(
                ctx.colors.success,
                format!("{} {}", result.files_fixed, label),
            ));
        }
        if result.warnings > 0 {
            parts.push(colored(
                ctx.colors.warning,
                format!("{} warnings", result.warnings),
            ));
        }
    }

    if result.errors > 0 {
        parts.push(colored(
            ctx.colors.error,
            format!("{} errors", result.errors),
        ));
    }
    if result.skipped > 0 {
        parts.push(colored(
            ctx.colors.info,
            format!("{} files skipped (see --verbose)", result.skipped),
        ));
    }
    if result.suppressed > 0 {
        let secrets = if result.suppressed_secrets > 0 {
            format!(" ({} secrets)", result.suppressed_secrets)
        } else {
            String::new()
        };
        parts.push(colored(
            ctx.colors.warning,
            format!(
                "{} problems suppressed by fini:ignore{}",
                result.suppressed, secrets
            ),
        ));
    }

    if !parts.is_empty() {
        println!();
        println!("{}", parts.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_path_display_escapes_newline_and_cr() {
        assert_eq!(
            safe_path_display(Path::new("evil\nFixed: other.txt")),
            "evil\\nFixed: other.txt"
        );
        assert_eq!(safe_path_display(Path::new("a\rb")), "a\\rb");
    }

    // issue #213: ESC / C1 in diff text must not reach the terminal raw.
    #[test]
    fn test_print_diff_to_escapes_esc_and_c1_but_keeps_tab() {
        let mut out = Vec::new();
        print_diff_to(&mut out, "f", "\x1b[8mhidden\n", "a\t\u{85}b\u{9f}\n").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            !text.contains('\u{1B}') && !text.chars().any(|c| ('\u{80}'..='\u{9F}').contains(&c)),
            "raw control char leaked: {text:?}"
        );
        assert!(text.contains("-\\x1b[8mhidden\n"), "{text:?}");
        assert!(text.contains("+a\t\\u{85}b\\u{9f}\n"), "{text:?}");
    }

    // issue #229: path labels (diff headers, Error:/Fixed:, walk errors) must
    // not carry raw ESC / C1 either.
    #[test]
    fn test_safe_path_display_escapes_esc_and_c1() {
        assert_eq!(
            safe_path_display(Path::new("a\u{1b}[8mb\u{9b}c")),
            "a\\x1b[8mb\\u{9b}c"
        );
    }

    #[test]
    fn test_diff_header_label_carries_no_raw_esc_or_c1() {
        let label = safe_path_display(Path::new("evil\u{1b}[2J\u{9b}x"));
        let mut out = Vec::new();
        print_diff_to(&mut out, &label, "a\n", "b\n").unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            !text.contains('\u{1B}') && !text.chars().any(|c| ('\u{80}'..='\u{9F}').contains(&c)),
            "raw control char leaked: {text:?}"
        );
        assert!(text.starts_with("--- evil\\x1b[2J\\u{9b}x\n"), "{text:?}");
    }

    const SECRET_LINE: &str = "password = \"hunter2hunter2\"";

    #[test]
    fn test_masked_diff_hides_changes_leading_blank_and_trailing_ws_before_secret() {
        let old = format!("\n{SECRET_LINE}  \n");
        let new = format!("{SECRET_LINE}\n");
        assert!(masked_diff_hides_changes(&old, &new));
    }

    #[test]
    fn test_masked_diff_hides_changes_secret_line_missing_eof_newline() {
        let old = format!("x = 1\n{SECRET_LINE}");
        let new = format!("x = 1\n{SECRET_LINE}\n");
        assert!(masked_diff_hides_changes(&old, &new));
    }

    #[test]
    fn test_masked_diff_hides_changes_lone_cr_inside_secret_line() {
        let old = format!("{SECRET_LINE}\rfoo\n");
        let new = format!("{SECRET_LINE}\nfoo\n");
        assert!(masked_diff_hides_changes(&old, &new));
    }

    #[test]
    fn test_masked_diff_hides_changes_false_without_secrets() {
        assert!(!masked_diff_hides_changes("a  \nb\n", "a\nb\n"));
    }

    #[test]
    fn test_safe_path_display_leaves_normal_path_unchanged() {
        assert_eq!(safe_path_display(Path::new("src/main.rs")), "src/main.rs");
    }
}
