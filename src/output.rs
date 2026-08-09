use crate::colors::Colors;
use crate::normalize::{mask_secret_lines, NormalizeConfig, NormalizeResult, ProblemKind};
use similar::{ChangeTag, TextDiff};
use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    Normal,
    Quiet,
    Diff,
}

pub struct Config {
    pub check_only: bool,
    pub output_mode: OutputMode,
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
        !self.check_only && self.output_mode != OutputMode::Diff
    }
}

pub struct OutputContext {
    pub mode: OutputMode,
    pub colors: Colors,
    pub verbose: bool,
    pub show_progress: bool,
    /// Mask secret-matching lines in diff output (mirrors detect_secrets)
    pub mask_secrets: bool,
}

impl OutputContext {
    pub fn new(
        mode: OutputMode,
        use_colors: bool,
        verbose: bool,
        show_progress: bool,
        mask_secrets: bool,
    ) -> Self {
        Self {
            mode,
            colors: Colors::new(use_colors),
            verbose,
            show_progress,
            mask_secrets,
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
        println!("{}", path.display());
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
            let (orig, new) = masked_pair(original, &result.content, ctx.mask_secrets);
            print_diff(&path.display().to_string(), &orig, &new);
        }
    } else {
        println!(
            "{}Error:{} {}",
            ctx.colors.error,
            ctx.colors.reset(),
            path.display()
        );

        if result.has_changes() {
            let orig_trimmed = original.trim_end_matches(['\n', '\r']);
            let orig_trailing_newlines = original[orig_trimmed.len()..]
                .chars()
                .filter(|&c| c == '\n')
                .count();
            let result_trimmed = result.content.trim_end_matches(['\n', '\r']);
            let result_trailing_newlines = result.content[result_trimmed.len()..]
                .chars()
                .filter(|&c| c == '\n')
                .count();

            if orig_trailing_newlines == 0 && result_trailing_newlines > 0 {
                println!("  - missing EOF newline");
            } else if orig_trailing_newlines > 1
                && result_trailing_newlines < orig_trailing_newlines
            {
                println!("  - extra trailing newline(s) removed");
            }

            for (i, orig_line) in original.lines().enumerate() {
                if orig_line.len() != orig_line.trim_end().len() {
                    println!("  - trailing whitespace at line {}", i + 1);
                }
            }
        }
    }

    for problem in &result.problems {
        match &problem.kind {
            ProblemKind::FullWidthSpace => {
                println!("  - full-width space at line {}", problem.line);
            }
            ProblemKind::LeadingBlankLines { count } => {
                println!("  - {} leading blank line(s)", count);
            }
            ProblemKind::ZeroWidthCharacter => {
                println!("  - zero-width character at line {}", problem.line);
            }
            ProblemKind::ExcessiveBlankLines { found, limit } => {
                println!(
                    "  - {} consecutive blank lines at line {} (limit: {})",
                    found, problem.line, limit
                );
            }
            ProblemKind::CodeBlockRemnant => {
                println!("  - code block remnant at line {}", problem.line);
            }
            ProblemKind::TodoComment => {
                println!("  - TODO comment at line {}", problem.line);
            }
            ProblemKind::FixmeComment => {
                println!("  - FIXME comment at line {}", problem.line);
            }
            ProblemKind::DebugCode { pattern } => {
                println!("  - debug code '{}' at line {}", pattern, problem.line);
            }
            ProblemKind::SecretPattern { hint } => {
                println!("  - potential secret ({}) at line {}", hint, problem.line);
            }
            ProblemKind::LongLine { length, limit } => {
                println!(
                    "  - line {} is too long ({} > {} chars)",
                    problem.line, length, limit
                );
            }
        }
    }
}

pub fn print_fix_result(
    path: &Path,
    original: &str,
    result: &NormalizeResult,
    ctx: &OutputContext,
) {
    match ctx.mode {
        OutputMode::Quiet => println!("{}", path.display()),
        OutputMode::Diff => {
            let (orig, new) = masked_pair(original, &result.content, ctx.mask_secrets);
            print_diff(&path.display().to_string(), &orig, &new);
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
                    path.display(),
                    problem.line
                );
            }
            println!(
                "{}Fixed:{} {}",
                ctx.colors.success,
                ctx.colors.reset(),
                path.display()
            );
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
        path.display()
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
        path.display()
    );
}

/// Mask secret-matching lines on both sides of a diff before printing, so the
/// diff path honors the same hint-only contract as check output (issue #44).
fn masked_pair<'a>(
    original: &'a str,
    content: &'a str,
    mask: bool,
) -> (Cow<'a, str>, Cow<'a, str>) {
    if mask {
        (
            Cow::Owned(mask_secret_lines(original)),
            Cow::Owned(mask_secret_lines(content)),
        )
    } else {
        (Cow::Borrowed(original), Cow::Borrowed(content))
    }
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
