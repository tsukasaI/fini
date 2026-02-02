pub mod normalize;
mod output;
pub mod walker;

pub use normalize::{normalize_content, NormalizeResult, Problem, ProblemKind};
pub use output::{Config, OutputMode, RunResult};
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
pub fn run(paths: &[String], config: &Config) -> io::Result<RunResult> {
    let mut result = RunResult {
        files_fixed: 0,
        files_with_problems: 0,
        warnings: 0,
    };

    for path in walk_paths(paths) {
        let path = path?;

        if let Err(e) = process_file(&path, config, &mut result) {
            if config.output_mode != OutputMode::Quiet {
                eprintln!("Error processing {}: {e}", path.display());
            }
        }
    }

    output::print_summary(&result, config);

    Ok(result)
}

fn process_file(path: &Path, config: &Config, result: &mut RunResult) -> io::Result<()> {
    let bytes = fs::read(path)?;

    // Skip empty files
    if bytes.is_empty() {
        return Ok(());
    }

    // Skip binary files
    if is_binary(&bytes) {
        return Ok(());
    }

    // Try to read as UTF-8
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return Ok(()), // Skip non-UTF-8 files
    };

    let normalize_result = normalize_content(&content);

    if !normalize_result.has_changes() {
        return Ok(());
    }

    let has_fullwidth = normalize_result
        .problems
        .iter()
        .any(|p| matches!(p.kind, ProblemKind::FullWidthSpace));

    if has_fullwidth {
        result.warnings += normalize_result
            .problems
            .iter()
            .filter(|p| matches!(p.kind, ProblemKind::FullWidthSpace))
            .count();
    }

    if config.check_only {
        result.files_with_problems += 1;
        output::print_check_result(path, &normalize_result, config);
    } else {
        fs::write(path, &normalize_result.content)?;
        result.files_fixed += 1;
        output::print_fix_result(path, &content, &normalize_result, config);
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
