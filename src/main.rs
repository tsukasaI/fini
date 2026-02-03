use std::process::ExitCode;

use clap::Parser;
use fini::{run, Config, NormalizeConfig, OutputMode};

#[derive(Parser)]
#[command(name = "fini")]
#[command(version, about = "A lightweight file normalization CLI tool")]
struct Cli {
    /// Target files or directories
    #[arg(required = true)]
    paths: Vec<String>,

    /// Check only (no modifications), exit 1 if problems found
    #[arg(short, long)]
    check: bool,

    /// Show changes in diff format
    #[arg(short, long)]
    diff: bool,

    /// Output only modified file names
    #[arg(short, long)]
    quiet: bool,

    /// Limit consecutive blank lines to N (0 = remove all blank lines)
    #[arg(long, value_name = "N")]
    max_blank_lines: Option<usize>,

    /// Keep zero-width characters (default: remove)
    #[arg(long)]
    keep_zero_width: bool,

    /// Keep leading blank lines (default: remove)
    #[arg(long)]
    keep_leading_blanks: bool,

    /// Remove code block remnants (```lang markers)
    #[arg(long)]
    fix_code_blocks: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let output_mode = if cli.quiet {
        OutputMode::Quiet
    } else if cli.diff {
        OutputMode::Diff
    } else {
        OutputMode::Normal
    };

    let normalize = NormalizeConfig {
        max_blank_lines: cli.max_blank_lines,
        remove_zero_width: !cli.keep_zero_width,
        remove_leading_blanks: !cli.keep_leading_blanks,
        fix_code_blocks: cli.fix_code_blocks,
    };

    let config = Config {
        check_only: cli.check,
        output_mode,
        normalize,
    };

    match run(&cli.paths, &config) {
        Ok(result) => {
            if config.check_only && result.has_problems() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}
