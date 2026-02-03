use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use fini::{
    check_editorconfig_conflicts, find_config_file, find_editorconfig, generate_init_file,
    load_config, merge_normalize_config, parse_editorconfig, run, CliNormalizeOptions, Config,
    FiniToml, OutputMode,
};

#[derive(Parser)]
#[command(name = "fini")]
#[command(version, about = "A lightweight file normalization CLI tool")]
struct Cli {
    /// Target files or directories
    #[arg(required_unless_present = "init")]
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

    /// Generate a template fini.toml configuration file
    #[arg(long)]
    init: bool,

    /// Specify config file path (overrides auto-discovery)
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Handle --init command
    if cli.init {
        return handle_init();
    }

    // Load configuration
    let toml_config = load_configuration(&cli.config, cli.quiet);

    // Check for editorconfig conflicts (informational warnings)
    if !cli.quiet {
        check_editorconfig_warnings();
    }

    // Build CLI options for merging
    let cli_options = build_cli_options(&cli);

    // Merge configurations: CLI > TOML > defaults
    let normalize =
        merge_normalize_config(&cli_options, toml_config.as_ref().map(|c| &c.normalize));

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

fn handle_init() -> ExitCode {
    match generate_init_file() {
        Ok(path) => {
            println!("Created {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::from(1)
        }
    }
}

fn load_configuration(explicit_path: &Option<PathBuf>, quiet: bool) -> Option<FiniToml> {
    let config_path = explicit_path.clone().or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|d| find_config_file(&d))
    });

    config_path.and_then(|p| match load_config(&p) {
        Ok(config) => {
            if !quiet {
                eprintln!("Using config: {}", p.display());
            }
            Some(config)
        }
        Err(e) => {
            eprintln!("Warning: Failed to load {}: {}", p.display(), e);
            None
        }
    })
}

fn check_editorconfig_warnings() {
    if let Some(editorconfig_path) = std::env::current_dir()
        .ok()
        .and_then(|d| find_editorconfig(&d))
    {
        if let Ok(settings) = parse_editorconfig(&editorconfig_path) {
            for warning in check_editorconfig_conflicts(&settings) {
                eprintln!("Warning: {}", warning);
            }
        }
    }
}

fn build_cli_options(cli: &Cli) -> CliNormalizeOptions {
    // Only set options that were explicitly provided on CLI.
    // Boolean flags in clap are always present (default false), so we
    // treat false as "not set" for proper merging with config file.
    CliNormalizeOptions {
        max_blank_lines: cli.max_blank_lines,
        keep_zero_width: cli.keep_zero_width.then_some(true),
        keep_leading_blanks: cli.keep_leading_blanks.then_some(true),
        fix_code_blocks: cli.fix_code_blocks.then_some(true),
    }
}
