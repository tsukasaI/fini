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

    println!(
        "{}Error:{} {}",
        ctx.colors.error,
        ctx.colors.reset(),
        path.display()
    );

    if result.has_changes() {
        if !original.ends_with('\n') && result.content.ends_with('\n') {
            println!("  - missing EOF newline");
        }

        for (i, (orig_line, _)) in original.lines().zip(result.content.lines()).enumerate() {
            if orig_line.len() != orig_line.trim_end().len() {
                println!("  - trailing whitespace at line {}", i + 1);
            }
        }
    }

    // Problems from normalization
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
            // Print warnings for full-width spaces
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
            parts.push(format!(
                "{}{} files fixed{}",
                ctx.colors.success,
                result.files_fixed,
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
