# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

fini is a Rust CLI tool for file normalization, designed as a finishing step for AI coding agents. It standardizes formatting (line endings, trailing whitespace, EOF newlines, zero-width characters, etc.) and detects human errors (TODOs, FIXMEs, debug code, secrets).

## Commands

```bash
cargo build                    # Build
cargo test                     # Run all tests (unit + integration)
cargo test <test_name>         # Run a single test by name
cargo test --lib               # Unit tests only
cargo test --test integration  # Integration tests only
cargo clippy                   # Lint
cargo fmt                      # Format
cargo fmt -- --check           # Format check (used in CI)
```

## Architecture

**Data flow**: CLI parsing (clap) → config loading (TOML + editorconfig) → config merging (CLI > TOML > defaults) → file walking (ignore crate, respects .gitignore) → parallel normalize (rayon) → sequential output/write → exit code.

Key design decisions:
- File reads and normalization run in **parallel** (rayon), but output and file writes are **sequential** for thread-safety and deterministic ordering.
- Config priority: CLI flags override TOML config, which overrides hardcoded defaults. See `config/merge.rs`.
- Binary detection uses null-byte check in first 8192 bytes (`lib.rs:is_binary`). Classification (empty/UTF-16 BOM/binary) reads only that prefix before the rest of the file is read.
- File writes are **atomic** (temp file + fsync + rename in the same directory, permissions preserved) and re-verify mtime/length just before writing; symlinks are skipped entirely. See `lib.rs:write_atomic`.
- Secret detections are hint-only everywhere: check output prints hints, and all diff paths mask secret-matching lines (`normalize::mask_secret_lines`). Suppressed secret detections (`fini:ignore`) always leave an stderr audit trail.

### Module map

- **`main.rs`** — CLI args via clap derive, handles `--init`/`--stdin` special modes, orchestrates config loading
- **`lib.rs`** — `run()` entry point: walks files, parallel processing, collects results, writes files
- **`normalize/mod.rs`** — `normalize_content()` pipeline: applies fixes then detections. Contains `NormalizeConfig`, `NormalizeResult`, `Problem` types and the bulk of unit tests
- **`normalize/fix.rs`** — Pure transformation functions (line endings, zero-width, trailing ws, EOF newline, blank lines, fullwidth spaces, code block remnants)
- **`normalize/detect.rs`** — Detection-only functions (TODO/FIXME markers, debug code patterns, secret regex patterns, line length). No auto-fix, only reporting
- **`config/`** — `file.rs` (upward search for fini.toml, stops at git root), `merge.rs` (three-way merge logic), `toml_schema.rs` (serde structs), `init.rs` (template generation), `editorconfig.rs` (conflict warnings)
- **`walker.rs`** — File traversal via `ignore` crate with custom exclude patterns (inverted gitignore-style globs)
- **`output.rs`** — Output modes (Normal/Quiet/Diff), summary stats, color-aware formatting
- **`colors.rs`** — ANSI color codes with NO_COLOR/TTY detection
- **`progress.rs`** — Progress bar (indicatif), only shown for 10+ files

### Normalization pipeline order

Fixes are applied in this sequence within `normalize_content()`:
1. Line ending normalization (CRLF/CR → LF)
2. Zero-width character removal (preserves BOM at position 0)
3. Code block remnant removal
4. Leading blank line removal
5. Consecutive blank line limiting
6. Fullwidth space conversion (U+3000 → space)
7. Trailing whitespace removal
8. EOF newline normalization

Code block remnant removal runs before the blank-line fixes so blank lines exposed by removing a fence (e.g. a fence at the top of the file, or blank lines split apart by a fence) are cleaned up in the same pass — otherwise `fini` fix output could fail an immediate `fini --check` (non-idempotent output).

Then detections run: TODOs, FIXMEs, debug code, secrets, line length.

Finally, if any problems exist, `fini:ignore` / `fini:ignore-next-line` directives are parsed from the normalized content and matching problems are filtered out (post-filter in `normalize/ignore.rs`).

### Testing conventions

- Unit tests live inline in each module (especially extensive in `normalize/mod.rs`)
- Integration tests in `tests/integration.rs` test the CLI binary end-to-end using `tempfile` for isolated directories
- Tests are organized by feature/issue area, not a clean phase sequence — early sections carry leftover "Phase N" labels from an earlier numbering scheme (reused out of order, e.g. two unrelated "Phase 3" sections), while later sections drop phase numbers entirely in favor of issue-referenced headers (e.g. "Atomic Writes & Symlinks (issue #35)")
