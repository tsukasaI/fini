use crate::colors::Colors;
use crate::normalize::{mask_secret_lines, NormalizeConfig, NormalizeResult, Problem, ProblemKind};
use similar::{ChangeTag, TextDiff};
use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;

/// Escapes `\r` and `\n` so `s` is safe to print as a standalone output
/// line. A string containing either would otherwise forge extra output
/// lines (issue #87) - e.g. `--quiet` mode's one-path-per-line contract,
/// which scripts parse, or a fake "Fixed: <other-file>" line spoofing a
/// result for a file that was never touched. Used both for a path's own
/// `Display` form (`safe_path_display`) and for error text that embeds a
/// path (e.g. a walk error from the `ignore` crate), since by the time
/// that text reaches us it's already a flattened string, not a `Path`.
pub fn escape_line_breaks(s: &str) -> String {
    s.replace('\r', "\\r").replace('\n', "\\n")
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
    pub mode: OutputMode,
    pub colors: Colors,
    pub verbose: bool,
    pub show_progress: bool,
}

impl OutputContext {
    pub fn new(mode: OutputMode, use_colors: bool, verbose: bool, show_progress: bool) -> Self {
        Self {
            mode,
            colors: Colors::new(use_colors),
            verbose,
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
) {
    if ctx.mode == OutputMode::Quiet {
        println!("{}", safe_path_display(path));
        return;
    }

    if ctx.mode == OutputMode::Diff {
        // Mirrors print_fix_result's Diff branch: masked the same way
        // (issue #44's contract applies to every diff path, not just fix
        // mode). Unlike print_fix_result, we don't return early: detection-
        // only problems (TODO/FIXME/debug/secret/long-line) never change
        // result.content, so there's no diff to show them in — they're only
        // visible via the problem list below, which must still run. When
        // there IS a content diff, skip the empty `---`/`+++` header instead
        // of printing a diff with no body.
        if result.has_changes() {
            let (orig, new) = masked_pair(original, &result.content);
            print_diff(&safe_path_display(path), &orig, &new);
        }
    } else {
        println!(
            "{}Error:{} {}",
            ctx.colors.error,
            ctx.colors.reset(),
            safe_path_display(path)
        );

        if result.has_changes() {
            let stdout = io::stdout();
            print_change_summary_to(&mut stdout.lock(), original, &result.content)
                .expect("failed to write to stdout");
        }
    }

    let stdout = io::stdout();
    // Panicking on a failed stdout write matches println!'s historical behavior
    print_problems_to(&mut stdout.lock(), &result.problems).expect("failed to write to stdout");
}

/// Prints the content-diff-derived summary lines (line endings, missing EOF
/// newline, extra trailing newlines, trailing whitespace) that Normal-mode
/// check output shows in place of a diff. Shared by file-mode check output
/// (to stdout) and `--stdin --check` without `--diff` (to stderr, since
/// stdin's stdout is reserved for normalized content only - issue #38) -
/// both need these bullets since fix-only problems never populate
/// `result.problems` (issue #79).
pub fn print_change_summary_to<W: Write>(
    w: &mut W,
    original: &str,
    result_content: &str,
) -> io::Result<()> {
    // A lone `\r` is itself a line ending that normalization collapses to
    // `\n` (see normalize::fix::normalize_line_endings), but str::lines()
    // doesn't split on it and trim_end_matches(['\n', '\r']) eats it as
    // trailing "whitespace" — so counting newlines and walking lines against
    // the raw `original` misattributes a CR-only file's EOF/trailing-
    // whitespace bullets. Normalize line endings first so those bullets only
    // fire for a change distinct from the line-ending one reported below
    // (issue #83).
    let original_lf = original.replace("\r\n", "\n").replace('\r', "\n");

    let orig_trimmed = original_lf.trim_end_matches('\n');
    let orig_trailing_newlines = original_lf[orig_trimmed.len()..]
        .chars()
        .filter(|&c| c == '\n')
        .count();
    let result_trimmed = result_content.trim_end_matches('\n');
    let result_trailing_newlines = result_content[result_trimmed.len()..]
        .chars()
        .filter(|&c| c == '\n')
        .count();

    if orig_trailing_newlines == 0 && result_trailing_newlines > 0 {
        writeln!(w, "  - missing EOF newline")?;
    } else if orig_trailing_newlines > 1 && result_trailing_newlines < orig_trailing_newlines {
        writeln!(w, "  - extra trailing newline(s) removed")?;
    }

    if original.contains('\r') {
        writeln!(w, "  - CRLF/CR line endings normalized to LF")?;
    }

    for (i, orig_line) in original_lf.lines().enumerate() {
        // Must match remove_trailing_whitespace's own trim set (ASCII space
        // and tab only) - str::trim_end() also strips other Unicode
        // whitespace (e.g. U+00A0 NBSP), which the fixer never touches, so
        // using it here reported "trailing whitespace" for lines fix mode
        // wouldn't actually change (issue #94).
        if orig_line.len() != orig_line.trim_end_matches([' ', '\t']).len() {
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
) {
    match ctx.mode {
        OutputMode::Quiet => println!("{}", safe_path_display(path)),
        OutputMode::Diff => {
            let (orig, new) = masked_pair(original, &result.content);
            print_diff(&safe_path_display(path), &orig, &new);
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

pub fn print_checked(path: &Path, ctx: &OutputContext) {
    if ctx.mode == OutputMode::Quiet {
        return;
    }
    println!(
        "{}Checked:{} {}",
        ctx.colors.info,
        ctx.colors.reset(),
        safe_path_display(path)
    );
}

pub fn print_skipped(path: &Path, reason: &str, ctx: &OutputContext) {
    if ctx.mode == OutputMode::Quiet {
        return;
    }
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
fn masked_pair<'a>(original: &'a str, content: &'a str) -> (Cow<'a, str>, Cow<'a, str>) {
    (
        Cow::Owned(mask_secret_lines(original)),
        Cow::Owned(mask_secret_lines(content)),
    )
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
                write!(w, "{sign}{change}")?;
            }
        }
    }
    Ok(())
}

pub fn print_summary(result: &RunResult, config: &Config, ctx: &OutputContext) {
    if ctx.mode == OutputMode::Quiet {
        return;
    }

    let mut parts = vec![];

    if config.check_only {
        if result.files_with_problems > 0 {
            parts.push(format!(
                "{}{} files with problems{}",
                ctx.colors.error,
                result.files_with_problems,
                ctx.colors.reset()
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
            parts.push(format!(
                "{}{} {}{}",
                ctx.colors.success,
                result.files_fixed,
                label,
                ctx.colors.reset()
            ));
        }
        if result.warnings > 0 {
            parts.push(format!(
                "{}{} warnings{}",
                ctx.colors.warning,
                result.warnings,
                ctx.colors.reset()
            ));
        }
    }

    if result.errors > 0 {
        parts.push(format!(
            "{}{} errors{}",
            ctx.colors.error,
            result.errors,
            ctx.colors.reset()
        ));
    }
    if result.skipped > 0 {
        parts.push(format!(
            "{}{} files skipped (see --verbose){}",
            ctx.colors.info,
            result.skipped,
            ctx.colors.reset()
        ));
    }
    if result.suppressed > 0 {
        let secrets = if result.suppressed_secrets > 0 {
            format!(" ({} secrets)", result.suppressed_secrets)
        } else {
            String::new()
        };
        parts.push(format!(
            "{}{} problems suppressed by fini:ignore{}{}",
            ctx.colors.warning,
            result.suppressed,
            secrets,
            ctx.colors.reset()
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

    #[test]
    fn test_safe_path_display_leaves_normal_path_unchanged() {
        assert_eq!(safe_path_display(Path::new("src/main.rs")), "src/main.rs");
    }
}
