pub mod colors;
pub mod config;
pub mod normalize;
mod output;
pub mod progress;
pub mod walker;

pub use colors::{should_use_colors, Colors};
pub use config::{
    check_editorconfig_conflicts, find_config_file, find_editorconfig, generate_init_file,
    load_config, merge_exclude_patterns, merge_normalize_config, parse_editorconfig,
    CliNormalizeOptions, ConfigError, FiniToml, NormalizeSection, FINI_TOML_TEMPLATE,
};
pub use normalize::{
    mask_secret_lines, normalize_content, NormalizeConfig, NormalizeResult, Problem, ProblemKind,
};
pub use output::{print_diff, print_diff_to, Config, OutputContext, OutputMode, RunResult};
pub use progress::ProgressReporter;
pub use walker::walk_paths;

use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::SystemTime;

const BINARY_CHECK_SIZE: usize = 8192;

/// Files are processed in batches of this size instead of all at once: each
/// batch is read+normalized in parallel, then applied (output/write/stats)
/// sequentially, before the next batch starts. This bounds peak memory to
/// roughly `CHUNK_SIZE * avg file size` (a changed file's original and
/// normalized content are both held until it's applied) instead of holding
/// every changed file in the run at once. 256 keeps that bound in the low
/// tens of MB for typical source files, while staying large enough that
/// per-chunk overhead (rayon dispatch, the sequential apply loop) is
/// negligible next to per-file I/O cost.
const CHUNK_SIZE: usize = 256;

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
    Clean {
        suppressed: Vec<Problem>,
    },
    Changed {
        original: String,
        result: NormalizeResult,
        /// mtime/length captured at read time, re-checked just before writing
        /// so an externally modified file is never overwritten (issue #35)
        modified: Option<SystemTime>,
        len: u64,
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
        skipped: 0,
        suppressed: 0,
        suppressed_secrets: 0,
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

    // Process files in fixed-size chunks: within a chunk, read+normalize run in
    // parallel and are collected (rayon's par_iter().collect() over a slice
    // preserves index order); then that chunk's results are applied (output +
    // writes + stats) sequentially before the next chunk starts. This keeps
    // walk order intact across chunk boundaries while bounding how much
    // changed-file content is held in memory at once (see CHUNK_SIZE).
    for chunk in file_paths.chunks(CHUNK_SIZE) {
        let outcomes: Vec<(&Path, FileOutcome)> = chunk
            .par_iter()
            .map(|path| {
                let outcome = process_file(path, &config.normalize);
                progress.inc();
                (path.as_path(), outcome)
            })
            .collect();

        for (path, outcome) in &outcomes {
            match outcome {
                FileOutcome::Skipped { reason } => {
                    // Empty files are trivially "done"; only count skips that mean
                    // "this file was not inspected" (binary, non-UTF-8, symlink)
                    if *reason != "empty" {
                        result.skipped += 1;
                    }
                    if ctx.verbose {
                        output::print_skipped(path, reason, ctx);
                    }
                }
                FileOutcome::Clean { suppressed } => {
                    record_suppressed(path, suppressed, &mut result);
                    if ctx.verbose {
                        output::print_checked(path, ctx);
                    }
                }
                FileOutcome::Changed {
                    original,
                    result: normalize_result,
                    modified,
                    len,
                } => {
                    record_suppressed(path, &normalize_result.suppressed, &mut result);
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
                            if config.should_write() {
                                if let Err(e) =
                                    write_atomic(path, &normalize_result.content, *modified, *len)
                                {
                                    eprintln!("Error writing {}: {e}", path.display());
                                    result.errors += 1;
                                    continue;
                                }
                            }
                            // Counts files that changed whether or not they were
                            // actually written — --diff previews without writing,
                            // and print_summary picks the wording accordingly.
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
    }

    progress.finish();

    output::print_summary(&result, config, ctx);

    Ok(result)
}

/// Process a single file: read, validate, normalize. Pure computation, no output.
fn process_file(path: &Path, normalize_config: &NormalizeConfig) -> FileOutcome {
    // Refuse symlinks outright: reading through one and writing back would
    // rewrite a target that may live outside the walked tree (issue #35)
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return FileOutcome::Skipped { reason: "symlink" }
        }
        Ok(_) => {}
        Err(e) => return FileOutcome::Error(e),
    }

    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return FileOutcome::Error(e),
    };
    let (modified, len) = match file.metadata() {
        // modified() is unsupported on some platforms; degrade to length-only
        // comparison there rather than failing every file
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(e) => return FileOutcome::Error(e),
    };

    // Classify from the first 8 KiB before reading the rest, so huge binaries
    // are never fully buffered just to be skipped (issue #39)
    let mut bytes = Vec::with_capacity(len.min(BINARY_CHECK_SIZE as u64) as usize);
    if let Err(e) = Read::by_ref(&mut file)
        .take(BINARY_CHECK_SIZE as u64)
        .read_to_end(&mut bytes)
    {
        return FileOutcome::Error(e);
    }

    if bytes.is_empty() {
        return FileOutcome::Skipped { reason: "empty" };
    }

    // A UTF-16 BOM means text in an encoding fini doesn't process; report the
    // skip reason accurately instead of lumping it in with "binary" (issue #40)
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return FileOutcome::Skipped {
            reason: "UTF-16 (unsupported encoding)",
        };
    }

    if is_binary(&bytes) {
        return FileOutcome::Skipped { reason: "binary" };
    }

    if let Err(e) = file.read_to_end(&mut bytes) {
        return FileOutcome::Error(e);
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
        return FileOutcome::Clean {
            suppressed: normalize_result.suppressed,
        };
    }

    FileOutcome::Changed {
        original: content,
        result: normalize_result,
        modified,
        len,
    }
}

/// Count suppressed problems and leave an stderr audit trail for suppressed
/// secrets — even in --quiet mode — so `fini:ignore secret` can be reviewed,
/// not just trusted (issue #46).
fn record_suppressed(path: &Path, suppressed: &[Problem], result: &mut RunResult) {
    result.suppressed += suppressed.len();
    for problem in suppressed {
        if let ProblemKind::SecretPattern { hint } = &problem.kind {
            result.suppressed_secrets += 1;
            eprintln!(
                "Warning: {}:{} potential secret ({hint}) suppressed by fini:ignore",
                path.display(),
                problem.line
            );
        }
    }
}

/// Atomically replace `path` with `content`: write to a temp file in the same
/// directory, fsync, then rename — a crash mid-write can never truncate the
/// original (issue #35). Refuses to write if the file was modified or replaced
/// by a symlink between the parallel read phase and this sequential write.
fn write_atomic(
    path: &Path,
    content: &str,
    read_modified: Option<SystemTime>,
    read_len: u64,
) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(
            "replaced by a symlink since it was read; not writing",
        ));
    }
    // mtime granularity varies by filesystem; length is the cheap second signal
    if meta.len() != read_len || meta.modified().ok() != read_modified {
        return Err(io::Error::other(
            "changed on disk since it was read; not writing (re-run fini)",
        ));
    }
    // Renaming over a read-only file succeeds (directory permissions govern
    // rename); refuse explicitly to preserve fs::write's permission semantics
    if meta.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "permission denied (read-only file)",
        ));
    }

    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::Builder::new().prefix(".fini-").tempfile_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.as_file().sync_all()?;
    // tempfile creates 0600 on unix; carry over the original's permissions
    // (owner/xattrs are not preserved — a documented limitation of rename-based writes)
    tmp.as_file().set_permissions(meta.permissions())?;
    tmp.persist(path).map(|_| ()).map_err(|e| e.error)
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
        let mut content = vec![b'a'; 8000];
        content.push(0);
        content.extend(vec![b'b'; 1000]);
        assert!(is_binary(&content));
    }

    #[test]
    fn test_binary_null_after_8192_bytes_not_detected() {
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
