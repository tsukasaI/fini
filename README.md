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
brew install tsukasaI/tap/fini
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
-v, --verbose           Show all processed files (including clean ones)
--stdin                 Read from stdin, output to stdout
--color                 Force colored output
--no-color              Disable colored output
--no-progress           Hide progress bar
--max-blank-lines <N>   Limit consecutive blank lines to N
--keep-zero-width       Keep zero-width characters (default: remove)
--keep-leading-blanks   Keep leading blank lines (default: remove)
--fix-code-blocks       Remove code block remnants (```lang markers)
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

Searches upward from current directory, stops at git root.

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

## Skipped

- Binary files (null bytes in first 8KB)
- Empty files
- Hidden files (`.foo`)
- `.git/` directory
- `.gitignore` patterns

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
- uses: tsukasaI/fini@v1
```

### Options

```yaml
- uses: tsukasaI/fini@v1
  with:
    files: 'src/ tests/'         # Files/directories to check (default: .)
    check: 'true'                # Check mode, fail if issues found (default: true)
    version: 'v0.3.0'            # Specific version (default: latest)
    verify-attestation: 'false'  # Verify SLSA build provenance via `gh attestation verify`
                                 # (default: false). Set to 'true' once your pinned fini
                                 # version has attestations (releases after v0.3.0).
```

## VS Code Extension

See [editors/vscode](./editors/vscode) for a VS Code extension that runs fini on save.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Problems found (`--check`) or error |

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
