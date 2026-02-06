use crate::colors::Colors;
use crate::normalize::{NormalizeConfig, NormalizeResult, ProblemKind};
use similar::{ChangeTag, TextDiff};
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
}

impl RunResult {
    pub fn has_problems(&self) -> bool {
        self.files_with_problems > 0
    }
}

pub fn print_check_result(
    path: &Path,
    result: &NormalizeResult,
    _config: &Config,
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

    if result.original != result.content {
        // Check what kind of changes were made
        if !result.original.ends_with('\n') && result.content.ends_with('\n') {
            println!("  - missing EOF newline");
        }

        // Check for trailing whitespace
        for (i, (orig_line, _)) in result
            .original
            .lines()
            .zip(result.content.lines())
            .enumerate()
        {
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
            // Phase 3: Human Error Prevention
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
    _config: &Config,
    ctx: &OutputContext,
) {
    match ctx.mode {
        OutputMode::Quiet => println!("{}", path.display()),
        OutputMode::Diff => print_diff(&path.display().to_string(), original, &result.content),
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

pub fn print_diff(label: &str, original: &str, content: &str) {
    let diff = TextDiff::from_lines(original, content);

    println!("--- {label}");
    println!("+++ {label}");

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            println!();
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                print!("{sign}{change}");
            }
        }
    }
}

pub fn print_summary(result: &RunResult, config: &Config, ctx: &OutputContext) {
    if ctx.mode == OutputMode::Quiet {
        return;
    }

    if config.check_only {
        if result.files_with_problems > 0 {
            println!();
            println!(
                "{}{} files with problems{}",
                ctx.colors.error,
                result.files_with_problems,
                ctx.colors.reset()
            );
        }
    } else if result.files_fixed > 0 || result.warnings > 0 {
        println!();
        let mut parts = vec![];
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
        println!("{}", parts.join(", "));
    }
}
