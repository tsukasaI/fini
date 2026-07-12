pub mod colors;
pub mod config;
pub mod normalize;
mod output;
pub mod progress;
pub mod walker;

pub use colors::{should_use_colors, Colors};
pub use config::{
    check_editorconfig_conflicts, find_config_file, find_editorconfig, generate_init_file,
    load_config, merge_normalize_config, parse_editorconfig, CliNormalizeOptions, ConfigError,
    FiniToml, NormalizeSection, FINI_TOML_TEMPLATE,
};
pub use normalize::{normalize_content, NormalizeConfig, NormalizeResult, Problem, ProblemKind};
pub use output::{print_diff, Config, OutputContext, OutputMode, RunResult};
pub use progress::ProgressReporter;
pub use walker::walk_paths;

use rayon::prelude::*;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const BINARY_CHECK_SIZE: usize = 8192;

/// Check if content is binary by looking for null bytes in first 8192 bytes
pub fn is_binary(content: &[u8]) -> bool {
    let check_len = content.len().min(BINARY_CHECK_SIZE);
    content[..check_len].contains(&0)
}

/// Result of processing a single file (pure data, no side effects)
enum FileOutcome {
    Skipped {
        reason: &'static str,
    },
    Clean,
    Changed {
        original: String,
        result: NormalizeResult,
    },
    Error(io::Error),
}

/// Main entry point: process all files in given paths
pub fn run(paths: &[String], config: &Config, ctx: &OutputContext) -> io::Result<RunResult> {
    let mut result = RunResult {
        files_fixed: 0,
        files_with_problems: 0,
        warnings: 0,
        errors: 0,
    };

    let mut file_paths = vec![];
    for entry in walk_paths(paths, &config.exclude_patterns)? {
        match entry {
            Ok(path) => file_paths.push(path),
            Err(e) => {
                eprintln!("Error walking path: {e}");
                result.errors += 1;
            }
        }
    }
    let progress = ProgressReporter::new(file_paths.len() as u64, ctx.show_progress);

    // Process files in parallel (read + normalize), collect results
    let outcomes: Vec<(PathBuf, FileOutcome)> = file_paths
        .into_par_iter()
        .map(|path| {
            let outcome = process_file(&path, &config.normalize);
            progress.inc();
            (path, outcome)
        })
        .collect();

    // Apply results sequentially (output + file writes + stats)
    for (path, outcome) in &outcomes {
        match outcome {
            FileOutcome::Skipped { reason } => {
                if ctx.verbose {
                    output::print_skipped(path, reason, ctx);
                }
            }
            FileOutcome::Clean => {
                if ctx.verbose {
                    output::print_checked(path, ctx);
                }
            }
            FileOutcome::Changed {
                original,
                result: normalize_result,
            } => {
                let fullwidth_count = normalize_result
                    .problems
                    .iter()
                    .filter(|p| matches!(p.kind, ProblemKind::FullWidthSpace))
                    .count();
                result.warnings += fullwidth_count;

                if config.check_only {
                    result.files_with_problems += 1;
                    output::print_check_result(path, original, normalize_result, ctx);
                } else {
                    if normalize_result.has_changes() {
                        if let Err(e) = fs::write(path, &normalize_result.content) {
                            eprintln!("Error writing {}: {e}", path.display());
                            result.errors += 1;
                            continue;
                        }
                        result.files_fixed += 1;
                    }
                    output::print_fix_result(path, original, normalize_result, ctx);
                }
            }
            FileOutcome::Error(e) => {
                eprintln!("Error processing {}: {e}", path.display());
                result.errors += 1;
            }
        }
    }

    progress.finish();

    output::print_summary(&result, config, ctx);

    Ok(result)
}

/// Process a single file: read, validate, normalize. Pure computation, no output.
fn process_file(path: &Path, normalize_config: &NormalizeConfig) -> FileOutcome {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => return FileOutcome::Error(e),
    };

    if bytes.is_empty() {
        return FileOutcome::Skipped { reason: "empty" };
    }

    if is_binary(&bytes) {
        return FileOutcome::Skipped { reason: "binary" };
    }

    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return FileOutcome::Skipped {
                reason: "non-UTF-8",
            }
        }
    };

    let normalize_result = normalize_content(&content, normalize_config);

    if !normalize_result.has_changes() && !normalize_result.has_detection_problems() {
        return FileOutcome::Clean;
    }

    FileOutcome::Changed {
        original: content,
        result: normalize_result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================
    // Phase 1.5: Binary Detection
    // ===========================================

    #[test]
    fn test_detect_binary_with_null_byte() {
        let content = b"hello\x00world";
        assert!(is_binary(content));
    }

    #[test]
    fn test_non_binary_text() {
        let content = b"hello world\nthis is text";
        assert!(!is_binary(content));
    }

    #[test]
    fn test_binary_check_within_8192_bytes() {
        // Null byte at position 8000 (within first 8192 bytes)
        let mut content = vec![b'a'; 8000];
        content.push(0);
        content.extend(vec![b'b'; 1000]);
        assert!(is_binary(&content));
    }

    #[test]
    fn test_binary_null_after_8192_bytes_not_detected() {
        // Null byte at position 9000 (after first 8192 bytes)
        let mut content = vec![b'a'; 9000];
        content.push(0);
        content.extend(vec![b'b'; 1000]);
        assert!(!is_binary(&content));
    }

    #[test]
    fn test_empty_content_not_binary() {
        let content: &[u8] = b"";
        assert!(!is_binary(content));
    }
}
