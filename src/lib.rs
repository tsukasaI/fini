#![warn(unnameable_types)]

mod colors;
mod config;
mod normalize;
mod output;
mod progress;
mod walker;

pub use colors::{should_use_colors, Colors};
pub use config::{
    check_editorconfig_conflicts, find_config_file, find_editorconfig, generate_init_file,
    load_config, merge_exclude_patterns, merge_normalize_config, parse_editorconfig,
    CliNormalizeOptions, ConfigError, EditorConfigSettings, FiniToml, NormalizeSection,
};
pub use normalize::{
    normalize_content, NormalizeConfig, NormalizeResult, Problem, ProblemKind, FIX_KIND_IGNORE_HINT,
};
pub use output::{
    escape_line_breaks, print_check_report_to, safe_path_display, warn_suppressed_secrets, Config,
    OutputContext, OutputMode, RunResult,
};

use walker::walk_paths;

use rayon::prelude::*;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
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
pub(crate) fn is_binary(content: &[u8]) -> bool {
    let check_len = content.len().min(BINARY_CHECK_SIZE);
    content[..check_len].contains(&0)
}

/// Why a file was not normalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    Empty,
    Symlink,
    TooLarge,
    Utf16,
    Binary,
    NonUtf8,
}

impl SkipReason {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Symlink => "symlink",
            Self::TooLarge => "too large (exceeds max-file-size)",
            Self::Utf16 => "UTF-16 (unsupported encoding)",
            Self::Binary => "binary",
            Self::NonUtf8 => "non-UTF-8",
        }
    }

    /// Empty files are trivially "done"; every other skip means "this file
    /// was not inspected".
    #[must_use]
    pub fn counts_as_skipped(self) -> bool {
        !matches!(self, Self::Empty)
    }
}

/// Result of processing a single file (pure data, no side effects)
enum FileOutcome {
    Skipped {
        reason: SkipReason,
    },
    Clean {
        suppressed: Vec<Problem>,
        ignores_only_fix_kinds: bool,
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

/// Files whose `fini:ignore` directives name only fix kinds: the first one
/// (in walk order) plus a count, so the once-per-run hint can
/// point at a file without listing them all.
#[derive(Default)]
struct FixKindHintSource {
    first: Option<PathBuf>,
    count: usize,
}

impl FixKindHintSource {
    fn note(&mut self, path: &Path) {
        if self.first.is_none() {
            self.first = Some(path.to_path_buf());
        }
        self.count = self.count.saturating_add(1);
    }

    fn message(&self) -> Option<String> {
        let first = self.first.as_ref()?;
        let more = match self.count - 1 {
            0 => String::new(),
            n => format!(" and {n} more"),
        };
        Some(format!(
            "{FIX_KIND_IGNORE_HINT} (in {}{more})",
            output::safe_path_display(first)
        ))
    }
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
    for entry in walk_paths(paths, &config.exclude_patterns, config.include_hidden)? {
        match entry {
            Ok(path) => file_paths.push(path),
            Err(e) => {
                eprintln!(
                    "Error walking path: {}",
                    output::escape_line_breaks(&e.to_string())
                );
                result.errors += 1;
            }
        }
    }

    // A target that resolves to zero files (an empty directory, or an
    // --exclude/config exclude pattern that - typo'd or not - matched
    // everything) otherwise exits 0 with no output at all, indistinguishable
    // from a successful run over real files (issue #92). Any walk error
    // already explains an empty result on its own, so this is additive only
    // when nothing else said why. Not gated on --quiet: a scan that silently
    // checked nothing is the kind of security-relevant surprise --quiet's
    // "only report actionable results" intent doesn't cover (same
    // reasoning as the issue #45 secret-detection-disabled warning).
    if file_paths.is_empty() && result.errors == 0 {
        eprintln!(
            "Warning: no files matched to scan (empty target, or every file was filtered by .gitignore/hidden-file rules or --exclude/config exclude patterns)"
        );
    }

    let mut fix_kind_hint = FixKindHintSource::default();
    let bar = progress::progress_bar(file_paths.len() as u64, ctx.show_progress);

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
                bar.inc(1);
                (path.as_path(), outcome)
            })
            .collect();

        for (path, outcome) in &outcomes {
            apply_outcome(path, outcome, config, ctx, &mut result, &mut fix_kind_hint);
        }
    }

    bar.finish_and_clear();

    // Advisory only, so --quiet silences it (unlike the secret-suppression
    // audit trail, which is security-relevant and always printed).
    if config.output_mode != OutputMode::Quiet {
        if let Some(hint) = fix_kind_hint.message() {
            eprintln!("{hint}");
        }
    }

    output::print_summary(&result, config, ctx);

    Ok(result)
}

/// Apply one file's outcome sequentially: output, writes, stats.
fn apply_outcome(
    path: &Path,
    outcome: &FileOutcome,
    config: &Config,
    ctx: &OutputContext,
    result: &mut RunResult,
    fix_kind_hint: &mut FixKindHintSource,
) {
    match outcome {
        FileOutcome::Skipped { reason } => {
            if reason.counts_as_skipped() {
                result.skipped += 1;
            }
            if ctx.verbose {
                output::print_skipped(path, reason.label(), ctx);
            }
        }
        FileOutcome::Clean {
            suppressed,
            ignores_only_fix_kinds,
        } => {
            record_suppressed(path, suppressed, result);
            if *ignores_only_fix_kinds {
                fix_kind_hint.note(path);
            }
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
            record_suppressed(path, &normalize_result.suppressed, result);
            if normalize_result.ignores_only_fix_kinds {
                fix_kind_hint.note(path);
            }
            let fullwidth_count = normalize_result
                .problems
                .iter()
                .filter(|p| matches!(p.kind, ProblemKind::FullWidthSpace))
                .count();
            result.warnings += fullwidth_count;

            if config.check_only {
                result.files_with_problems += 1;
                output::print_check_result(
                    path,
                    original,
                    normalize_result,
                    ctx,
                    config.normalize.remove_zero_width,
                );
                return;
            }
            if normalize_result.has_changes() {
                if config.should_write() {
                    if let Err(e) = write_atomic(path, &normalize_result.content, *modified, *len) {
                        eprintln!("Error writing {}: {e}", output::safe_path_display(path));
                        result.errors += 1;
                        return;
                    }
                }
                // Counts files that changed whether or not they were
                // actually written; --diff previews without writing,
                // and print_summary picks the wording accordingly.
                result.files_fixed += 1;
            }
            output::print_fix_result(
                path,
                original,
                normalize_result,
                ctx,
                config.normalize.remove_zero_width,
            );
        }
        FileOutcome::Error(e) => {
            eprintln!("Error processing {}: {e}", output::safe_path_display(path));
            result.errors += 1;
        }
    }
}

/// Read `reader` as UTF-8 text, applying the gates shared by file and stdin
/// input: empty, UTF-16 BOM, binary (NUL), `max_file_size`, non-UTF-8.
///
/// Reads at most `max_file_size + 1` bytes, so an oversize or unbounded
/// source is never buffered past the limit. The outer `Err` is an I/O
/// failure; the inner `Err` is a skip decision.
pub fn read_gated(
    mut reader: impl Read,
    max_file_size: u64,
) -> io::Result<Result<String, SkipReason>> {
    // Classify from the first 8 KiB before reading the rest, so huge binaries
    // are never fully buffered just to be skipped (issue #39). Capped at the
    // limit + 1 as well so a tiny limit is not overshot by the prefix read.
    let prefix_len = (BINARY_CHECK_SIZE as u64).min(max_file_size.saturating_add(1));
    let mut bytes = Vec::new();
    Read::by_ref(&mut reader)
        .take(prefix_len)
        .read_to_end(&mut bytes)?;

    if bytes.is_empty() {
        return Ok(Err(SkipReason::Empty));
    }

    // A UTF-16 BOM means text in an encoding fini doesn't process; report the
    // skip reason accurately instead of lumping it in with "binary" (issue #40)
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Ok(Err(SkipReason::Utf16));
    }

    if is_binary(&bytes) {
        return Ok(Err(SkipReason::Binary));
    }

    // Bound this read too: for files the size gate only checked the length at
    // stat time, and a file can grow between that stat and this read (an
    // actively appended log, or a hostile repo); stdin has no length at all.
    // A security control must hold on every path, not just the common one.
    // +1 so input of exactly the limit still isn't flagged, but anything
    // beyond it is.
    let remaining_allowed = max_file_size
        .saturating_add(1)
        .saturating_sub(bytes.len() as u64);
    Read::by_ref(&mut reader)
        .take(remaining_allowed)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_file_size {
        return Ok(Err(SkipReason::TooLarge));
    }

    Ok(String::from_utf8(bytes).map_err(|_| SkipReason::NonUtf8))
}

/// Process a single file: read, validate, normalize. Pure computation, no output.
fn process_file(path: &Path, normalize_config: &NormalizeConfig) -> FileOutcome {
    // Refuse symlinks outright: reading through one and writing back would
    // rewrite a target that may live outside the walked tree (issue #35)
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return FileOutcome::Skipped {
                reason: SkipReason::Symlink,
            }
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

    // Size gate before any read - see NormalizeConfig::max_file_size for why
    // (issue #103).
    if len > normalize_config.max_file_size {
        return FileOutcome::Skipped {
            reason: SkipReason::TooLarge,
        };
    }

    let content = match read_gated(&mut file, normalize_config.max_file_size) {
        Ok(Ok(content)) => content,
        Ok(Err(reason)) => return FileOutcome::Skipped { reason },
        Err(e) => return FileOutcome::Error(e),
    };

    let normalize_result = normalize_content(&content, normalize_config);

    if !normalize_result.has_changes() && !normalize_result.has_detection_problems() {
        return FileOutcome::Clean {
            suppressed: normalize_result.suppressed,
            ignores_only_fix_kinds: normalize_result.ignores_only_fix_kinds,
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
    result.suppressed_secrets +=
        output::warn_suppressed_secrets(&output::safe_path_display(path), suppressed);
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
    // rename); refuse explicitly to preserve fs::write's permission
    // semantics. `meta.permissions().readonly()` (mode bits) alone isn't
    // the same question as "can the current user write this file": a file
    // owned by someone else with mode 0644 has its write bit set but still
    // isn't writable by us, and `readonly()` would say false, letting the
    // rename silently replace another user's file (issue #100). Keep the
    // mode-bit check too (not replace it) - root can open a 0444 file for
    // write (CAP_DAC_OVERRIDE), and this guard existed specifically to
    // refuse rewriting a read-only file regardless of who's running fini.
    if meta.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "permission denied (read-only file)",
        ));
    }
    if let Err(e) = fs::OpenOptions::new().write(true).open(path) {
        return Err(io::Error::new(
            e.kind(),
            format!("cannot open for write: {e}"),
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

    #[test]
    fn test_fix_kind_hint_source_message() {
        let mut source = FixKindHintSource::default();
        assert_eq!(source.message(), None);

        source.note(Path::new("a.txt"));
        assert_eq!(
            source.message().unwrap(),
            format!("{FIX_KIND_IGNORE_HINT} (in a.txt)")
        );

        source.note(Path::new("b.txt"));
        source.note(Path::new("c.txt"));
        assert_eq!(
            source.message().unwrap(),
            format!("{FIX_KIND_IGNORE_HINT} (in a.txt and 2 more)")
        );
    }

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
