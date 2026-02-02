use std::process::ExitCode;

use clap::Parser;
use fini::{run, Config, OutputMode};

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

    let config = Config {
        check_only: cli.check,
        output_mode,
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
