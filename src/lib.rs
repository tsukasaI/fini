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

use std::fs;
use std::io;
use std::path::Path;

const BINARY_CHECK_SIZE: usize = 8192;

/// Check if content is binary by looking for null bytes in first 8192 bytes
pub fn is_binary(content: &[u8]) -> bool {
    let check_len = content.len().min(BINARY_CHECK_SIZE);
    content[..check_len].contains(&0)
}

/// Main entry point: process all files in given paths
pub fn run(paths: &[String], config: &Config, ctx: &OutputContext) -> io::Result<RunResult> {
    let mut result = RunResult {
        files_fixed: 0,
        files_with_problems: 0,
        warnings: 0,
    };

    // Count files for progress bar (2-pass approach)
    let file_count: u64 = walk_paths(paths).filter_map(|r| r.ok()).count() as u64;

    let progress = ProgressReporter::new(file_count, ctx.show_progress);

    for path in walk_paths(paths) {
        let path = path?;

        // Update progress bar message with current file name
        if let Some(name) = path.file_name() {
            progress.set_message(&name.to_string_lossy());
        }

        if let Err(e) = process_file(&path, config, &mut result, ctx) {
            if ctx.mode != OutputMode::Quiet {
                eprintln!("Error processing {}: {e}", path.display());
            }
        }

        progress.inc();
    }

    progress.finish();

    output::print_summary(&result, config, ctx);

    Ok(result)
}

fn process_file(
    path: &Path,
    config: &Config,
    result: &mut RunResult,
    ctx: &OutputContext,
) -> io::Result<()> {
    let bytes = fs::read(path)?;

    // Skip empty files
    if bytes.is_empty() {
        if ctx.verbose {
            output::print_skipped(path, "empty", ctx);
        }
        return Ok(());
    }

    // Skip binary files
    if is_binary(&bytes) {
        if ctx.verbose {
            output::print_skipped(path, "binary", ctx);
        }
        return Ok(());
    }

    // Try to read as UTF-8
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            if ctx.verbose {
                output::print_skipped(path, "non-UTF-8", ctx);
            }
            return Ok(());
        }
    };

    let normalize_result = normalize_content(&content, &config.normalize);

    // Check for detection-only problems (these don't change content)
    let has_detection_problems = normalize_result
        .problems
        .iter()
        .any(|p| p.kind.is_detection_only());

    if !normalize_result.has_changes() && !has_detection_problems {
        // No changes and no detection problems
        if ctx.verbose {
            output::print_checked(path, ctx);
        }
        return Ok(());
    }

    let fullwidth_count = normalize_result
        .problems
        .iter()
        .filter(|p| matches!(p.kind, ProblemKind::FullWidthSpace))
        .count();
    result.warnings += fullwidth_count;

    if config.check_only {
        result.files_with_problems += 1;
        output::print_check_result(path, &normalize_result, config, ctx);
    } else {
        // Only write if content changed (detection problems don't modify content)
        if normalize_result.has_changes() {
            fs::write(path, &normalize_result.content)?;
            result.files_fixed += 1;
        }
        // Print fix result if there were changes or detection problems
        if normalize_result.has_changes() || has_detection_problems {
            output::print_fix_result(path, &content, &normalize_result, config, ctx);
        }
    }

    Ok(())
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
