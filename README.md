# fini

[![CI](https://github.com/tsukasaI/fini/actions/workflows/ci.yaml/badge.svg)](https://github.com/tsukasaI/fini/actions/workflows/ci.yaml)
[![Crates.io](https://img.shields.io/crates/v/fini.svg)](https://crates.io/crates/fini)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A lightweight file normalization CLI tool for AI coding agents.

Standardizes file formatting as a finishing step after code editing.

## Installation

### Cargo
```bash
cargo install fini
```

### Nix
```bash
nix run github:tsukasaI/fini -- .
nix profile install github:tsukasaI/fini
```

### Homebrew
```bash
brew tap tsukasaI/fini https://github.com/tsukasaI/fini
brew install tsukasaI/fini/fini
```

### Pre-built binaries
Download from [GitHub Releases](https://github.com/tsukasaI/fini/releases).

### Pre-commit / Prek

Add to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/tsukasaI/fini
    rev: v0.3.0  # Use the latest release tag
    hooks:
      - id: fini        # Auto-fix mode
      # or
      - id: fini-check  # Check-only mode (for CI)
```

Note: Requires fini to be installed (`cargo install fini` or via Homebrew/Nix).

## Usage

```bash
fini .                    # Fix current directory
fini src/main.rs          # Fix specific file
fini --check .            # Check only, exit 1 if problems
fini --diff .             # Preview changes
fini --quiet .            # Output only filenames
fini -v .                 # Verbose: show all processed files
fini --init               # Generate fini.toml template
cat file.txt | fini --stdin  # Read from stdin, output to stdout
```

### Options

```
-c, --check             Check only (no modifications), exit 1 if problems found
-d, --diff              Show changes in diff format
-q, --quiet             Output only modified file names
-v, --verbose           Show all processed files (including clean ones)
--stdin                 Read from stdin, output to stdout
--color                 Force colored output
--no-color              Disable colored output
--no-progress           Hide progress bar
--max-blank-lines <N>   Limit consecutive blank lines to N
--keep-zero-width       Keep zero-width characters (default: remove)
--keep-leading-blanks   Keep leading blank lines (default: remove)
--fix-code-blocks       Remove code block remnants (```lang markers)
--no-detect-todos       Skip TODO comment detection
--no-detect-fixmes      Skip FIXME comment detection
--no-detect-debug       Skip debug code detection
--strict-debug          Include console.error/eprintln in debug code detection
--no-detect-secrets     Skip secret pattern detection
--max-line-length <N>   Maximum line length (warn if exceeded)
--exclude <PATTERN>     Exclude files matching glob pattern (repeatable)
--init                  Generate fini.toml configuration template
--config <PATH>         Use specific config file
```

## Configuration

Create `fini.toml` in your project root (or run `fini --init`):

```toml
# Exclude files matching these patterns (gitignore-style globs)
# exclude = ["vendor/", "node_modules/", "*.min.js"]

[normalize]
max_blank_lines = 2        # Limit consecutive blank lines
remove_zero_width = true   # Remove zero-width characters
remove_leading_blanks = true
fix_code_blocks = false    # Remove ``` markers
```

### Priority

CLI arguments > `fini.toml` > defaults

### Config Discovery

Searches upward from current directory, stops at git root. Outside a
git repository (no `.git` in any ancestor), only the current directory
is checked.

### .editorconfig

fini reads `.editorconfig` and warns if settings conflict with its fixed behaviors (always trims whitespace, always LF, always adds final newline).

## Features

| Rule | Description | Default |
|------|-------------|---------|
| EOF newline | Add `\n` if missing, normalize multiple trailing newlines | On |
| Line endings | CRLF/CR to LF | On |
| Trailing whitespace | Remove trailing spaces and tabs | On |
| Full-width spaces | Fix U+3000 to regular space (with warning) | On |
| Leading blank lines | Remove blank lines at file start | On |
| Zero-width characters | Remove ZWSP, ZWJ, ZWNJ, etc. (preserve BOM at start) | On |
| Consecutive blank lines | Limit to N blank lines (`--max-blank-lines`) | Off |
| Code block remnants | Remove ``` markers (`--fix-code-blocks`) | Off |

Files are written atomically (write to a temp file in the same directory,
then rename over the original), which breaks any other hard link to the
file - other links keep the pre-fix content, since rename only repoints the
one directory entry fini wrote to.

## Inline Ignore

Suppress detections per-line with `fini:ignore` directives. Works with any comment syntax.

```python
# TODO: intentional reminder fini:ignore
print("debug") # fini:ignore debug

# fini:ignore-next-line
API_KEY = "sk_test_example"
```

| Directive | Effect |
|-----------|--------|
| `fini:ignore` | Suppress all detections on this line |
| `fini:ignore todo,debug` | Suppress only listed kinds |
| `fini:ignore-next-line` | Suppress all detections on the next line |
| `fini:ignore-next-line secret` | Suppress only listed kinds on the next line |

Kind identifiers: `todo`, `fixme`, `debug`, `secret`, `line-length`, `fullwidth`, `zero-width`, `leading-blanks`, `blank-lines`, `code-block`

Everything on a line from `fini:ignore`/`fini:ignore-next-line` onward is
directive syntax, not code, so it's never scanned for TODO/FIXME markers or
counted as a detection.

Suppressions are counted in the run summary. Suppressed `secret` detections are
always reported on stderr with `file:line` (even with `--quiet`) so they remain
auditable.

## Secret detection and output

Secret detections are reported hint-only — the matched value is never printed.
`--diff` output masks lines matching a secret pattern
(`[line masked: potential …]`), but a secret that no pattern matches will appear
verbatim in a diff: be careful publishing CI logs that include `--diff` output.

Disabling secret detection via `fini.toml` (`detect_secrets = false`) prints a
warning to stderr that `--quiet` does not suppress.

Directory scans skip hidden files by default (see Skipped below), including
`.env` and `.github/workflows/*.yml`, two of the most common places a secret
ends up. `fini --check .` as a CI secret-detection gate therefore does not
cover them unless you also pass `--hidden`, or check those paths directly
(`fini --check .env`).

## Skipped

- Binary files (null bytes in first 8KB)
- UTF-16 and other non-UTF-8 text files (unsupported encodings)
- Symlinks (never followed or rewritten)
- Empty files
- Hidden files (`.foo`); pass `--hidden` to include them (`.git/` is still
  excluded either way). `--hidden` widens fini's own default, not your
  `.gitignore`/global excludes: a file your own ignore rules already hide
  (many developers' global gitignore lists `.env`) stays hidden regardless
- `.git/` directory
- `.gitignore` patterns

Skipped binary / non-UTF-8 / symlink files are counted in the run summary;
use `--verbose` to list them individually with the skip reason.

## Claude Code Integration

Add to `.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Edit|Write|NotebookEdit",
      "hooks": [{
        "type": "command",
        "command": "fini \"$TOOL_INPUT.file_path\""
      }]
    }]
  }
}
```

## GitHub Action

```yaml
- uses: tsukasaI/fini@v0.4.0
```

Pin to a release tag as above, or to a full commit SHA for supply-chain hardening.

The action runs `fini --check` by default. Note that fix mode (without `--check`)
never fails on detection-only problems (TODOs, debug code, secrets) — always use
`--check` when gating CI.

### Options

```yaml
- uses: tsukasaI/fini@v0.4.0
  with:
    files: 'src/ tests/'         # Files/directories to check (default: .)
    check: 'true'                # Check mode, fail if issues found (default: true)
    version: 'v0.3.0'            # Specific version (default: latest)
    verify-attestation: 'false'  # Verify SLSA build provenance via `gh attestation verify`
                                 # (defaults to true as of the version of this action that
                                 # ships this comment). The default `version: latest` always
                                 # resolves to an attested release, so this only needs
                                 # setting to 'false' if you pin `version` to a release
                                 # before v0.4.0, which predates attestations.
```

## VS Code Extension

See [editors/vscode](./editors/vscode) for a VS Code extension that runs fini on save.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success (no problems / fix completed) |
| 1 | Problems found (`--check`) |
| 2 | Runtime error (I/O failure, invalid config or exclude pattern) |

Fix mode (without `--check`) exits 0 even when detection-only problems
(TODOs, debug code, secrets) are reported — they are informational there.
Use `fini --check` as the CI gate.

On Unix, if fini's output pipe closes early (e.g. `fini --quiet dir | head`),
the process is killed by SIGPIPE rather than exiting with a code of its own
(the shell reports 141). In fix mode this stops processing at that point;
files not yet reached are left unprocessed.

## Development

### Pre-commit Hooks

This project uses [prek](https://github.com/j178/prek) for pre-commit hooks.

```bash
# Install prek
cargo install prek

# Install git hooks
prek install

# Run hooks manually
prek run --all-files
```

## License

MIT
