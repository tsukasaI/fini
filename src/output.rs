use crate::normalize::{NormalizeResult, ProblemKind};
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

pub fn print_check_result(path: &Path, result: &NormalizeResult, config: &Config) {
    if config.output_mode == OutputMode::Quiet {
        println!("{}", path.display());
        return;
    }

    println!("Error: {}", path.display());

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

    // Full-width spaces
    for problem in &result.problems {
        if matches!(problem.kind, ProblemKind::FullWidthSpace) {
            println!("  - full-width space at line {}", problem.line);
        }
    }
}

pub fn print_fix_result(
    path: &Path,
    original: &str,
    result: &NormalizeResult,
    config: &Config,
) {
    match config.output_mode {
        OutputMode::Quiet => {
            println!("{}", path.display());
        }
        OutputMode::Diff => {
            print_diff(path, original, &result.content);
        }
        OutputMode::Normal => {
            // Print warnings for full-width spaces
            for problem in &result.problems {
                if matches!(problem.kind, ProblemKind::FullWidthSpace) {
                    println!(
                        "Warning: {}:{} full-width space",
                        path.display(),
                        problem.line
                    );
                }
            }
            println!("Fixed: {}", path.display());
        }
    }
}

fn print_diff(path: &Path, original: &str, content: &str) {
    let diff = TextDiff::from_lines(original, content);

    println!("--- {}", path.display());
    println!("+++ {}", path.display());

    for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
        if idx > 0 {
            println!();
        }

        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };

                print!("{}{}", sign, change);
            }
        }
    }
}

pub fn print_summary(result: &RunResult, config: &Config) {
    if config.output_mode == OutputMode::Quiet {
        return;
    }

    if config.check_only {
        if result.files_with_problems > 0 {
            println!();
            println!("{} files with problems", result.files_with_problems);
        }
    } else if result.files_fixed > 0 || result.warnings > 0 {
        println!();
        let mut parts = vec![];
        if result.files_fixed > 0 {
            parts.push(format!("{} files fixed", result.files_fixed));
        }
        if result.warnings > 0 {
            parts.push(format!("{} warnings", result.warnings));
        }
        println!("{}", parts.join(", "));
    }
}
