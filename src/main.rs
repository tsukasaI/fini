use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use fini::{
    check_editorconfig_conflicts, find_config_file, find_editorconfig, generate_init_file,
    load_config, merge_normalize_config, normalize_content, parse_editorconfig, print_diff, run,
    should_use_colors, CliNormalizeOptions, Config, FiniToml, OutputContext, OutputMode,
};

#[derive(Parser)]
#[command(name = "fini")]
#[command(version, about = "A lightweight file normalization CLI tool")]
struct Cli {
    /// Target files or directories
    #[arg(required_unless_present_any = ["init", "stdin"])]
    paths: Vec<String>,

    /// Read input from stdin (output to stdout)
    #[arg(long)]
    stdin: bool,

    /// Check only (no modifications), exit 1 if problems found
    #[arg(short, long)]
    check: bool,

    /// Show changes in diff format
    #[arg(short, long)]
    diff: bool,

    /// Output only modified file names
    #[arg(short, long)]
    quiet: bool,

    /// Show all processed files (including clean ones)
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Force colored output
    #[arg(long)]
    color: bool,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Hide progress bar
    #[arg(long)]
    no_progress: bool,

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

    // Phase 3: Human Error Prevention
    /// Skip TODO comment detection
    #[arg(long)]
    no_detect_todos: bool,

    /// Skip FIXME comment detection
    #[arg(long)]
    no_detect_fixmes: bool,

    /// Skip debug code detection
    #[arg(long)]
    no_detect_debug: bool,

    /// Include console.error/eprintln in debug code detection
    #[arg(long)]
    strict_debug: bool,

    /// Skip secret pattern detection
    #[arg(long)]
    no_detect_secrets: bool,

    /// Maximum line length (warn if exceeded)
    #[arg(long, value_name = "N")]
    max_line_length: Option<usize>,

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

    // Handle --stdin command
    if cli.stdin {
        return handle_stdin(&cli);
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

    // Determine color, verbose, and progress settings
    // --quiet overrides --verbose
    let use_colors = should_use_colors(cli.color, cli.no_color);
    let verbose = cli.verbose && !cli.quiet;
    let show_progress = !cli.quiet && !cli.no_progress && std::io::stdout().is_terminal();

    let ctx = OutputContext::new(output_mode, use_colors, verbose, show_progress);

    match run(&cli.paths, &config, &ctx) {
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

fn handle_stdin(cli: &Cli) -> ExitCode {
    // Read from stdin
    let mut input = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut input) {
        eprintln!("Error reading stdin: {e}");
        return ExitCode::from(1);
    }

    // Build normalize config
    let cli_options = build_cli_options(cli);
    let normalize = merge_normalize_config(&cli_options, None);

    // Normalize content
    let result = normalize_content(&input, &normalize);

    // Check for detection-only problems
    let has_detection_problems = result.problems.iter().any(|p| p.kind.is_detection_only());

    if cli.check {
        // Check mode: exit 1 if there are changes or detection problems
        if result.has_changes() || has_detection_problems {
            if cli.diff {
                // Print diff to stderr so stdout stays clean
                print_diff("stdin", &input, &result.content);
            }
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
    }

    // Normal mode: output normalized content to stdout
    print!("{}", result.content);
    if let Err(e) = io::stdout().flush() {
        eprintln!("Error writing stdout: {e}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
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
        // Phase 3: Human Error Prevention
        no_detect_todos: cli.no_detect_todos.then_some(true),
        no_detect_fixmes: cli.no_detect_fixmes.then_some(true),
        no_detect_debug: cli.no_detect_debug.then_some(true),
        strict_debug: cli.strict_debug.then_some(true),
        no_detect_secrets: cli.no_detect_secrets.then_some(true),
        max_line_length: cli.max_line_length,
    }
}
