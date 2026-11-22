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

Finally, if any problems exist, `fini:ignore` / `fini:ignore-next-line` directives are parsed from the normalized content and matching detection problems are filtered out (post-filter in `normalize/ignore.rs`). The filter applies to detection kinds only: `ProblemKind::ignore_id()` is `None` for fix kinds, so they are never suppressed.

## Decided policy

Settled product decisions. A review finding that matches a line here is previously decided: settle it as a `note`, not a `needs-decision` issue, unless it shows data loss. Background: `audit/convergence-decisions.md`.

- The primary integration is each repository's pre-commit hook, not a per-edit agent hook: over 30 days of Claude Code transcripts, 1.4% of Write calls and almost no Edit calls needed a fix (`audit/convergence-decisions.md`). No global git hook is offered.
- Deterministic fixes (trailing whitespace, EOF newline, line endings) are fini's core job and stay, however rarely agents produce them: a program can guarantee them, an agent cannot.
- Secret detection and diff masking are best-effort hints, not a security boundary. fini does not promise to catch every secret shape or to mask every output path; a leak of a detected secret is an ordinary bug, not a boundary violation. Use a dedicated scanner (e.g. gitleaks) for gating.
- Invisible characters: remove U+200B, U+2060 and mid-file U+FEFF; keep ZWJ, ZWNJ, LRM and RLM, also between ASCII characters (#265, #278); report bidi controls U+202A to U+202E and U+2066 to U+2069 without rewriting (#279). Marker and debug detection also match the line stripped with the same set secret masking uses, `strip_mask_ignorable` (#277, #285).
- Exclude patterns: every argument inside the cwd, file or directory, is matched by its real path made relative to the cwd; a symlinked directory that resolves outside the cwd is also matched as typed; a file argument outside the cwd is matched by file name only. A matching directory argument is skipped, and skipped arguments are counted in the summary and listed under `--verbose` (#273, #274, #275, #282, #287).
- Audits run only before a release and file only medium or higher; lower findings stay in the audit report.

## Quality bar

The `review-gate` skill settles review findings by this table instead of its provenance rows. Inside the bar may become an issue by the usual tiers; outside is a `note` (`## Not filed`) whatever its kind.

| Finding | Bar |
|---|---|
| A fix loses or corrupts file content, or writes when a flag promises no write | inside |
| A crash, a wrong exit code, or output that contradicts README | inside |
| A detected secret printed raw by an output path (ordinary bug, not a boundary violation) | inside |
| A change that makes `--check` fail on common non-secret content (config names, placeholders, i18n labels) that no Decided policy line asks fini to report | inside |
| The change contradicts a Decided policy line | inside |
| Re-asks a question a Decided policy line answers | outside |
| A secret shape the patterns do not catch | outside |
| Wording, comments, test hardening, refactors, naming | outside |

When a finding matches both an inside and an outside row, the inside row wins only for data loss (the first row); otherwise a re-ask of a Decided policy line is outside.

A PR that closes issues for a goal ends its body with `Closed: N / Filed: M`: N issues it closes, M `needs-decision` and bug issues filed from its review. The goal converges when N > M for three goals in a row.

### Testing conventions

- Unit tests live inline in each module (especially extensive in `normalize/mod.rs`), except `normalize/fix.rs`, whose transformations are covered only indirectly via `normalize_content()` tests in `mod.rs`
- Integration tests in `tests/integration.rs` test the CLI binary end-to-end using `tempfile` for isolated directories
- Tests are organized by feature/issue area, not a clean phase sequence — early sections carry leftover "Phase N" labels from an earlier numbering scheme (reused out of order, e.g. two unrelated "Phase 3" sections), while later sections drop phase numbers entirely in favor of issue-referenced headers (e.g. "Atomic Writes & Symlinks (issue #35)")
