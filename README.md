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

## Usage

```bash
fini .                    # Fix current directory
fini src/main.rs          # Fix specific file
fini --check .            # Check only, exit 1 if problems
fini --diff .             # Preview changes
fini --quiet .            # Output only filenames
```

### Options

```
--max-blank-lines <N>   Limit consecutive blank lines to N
--keep-zero-width       Keep zero-width characters (default: remove)
--keep-leading-blanks   Keep leading blank lines (default: remove)
--fix-code-blocks       Remove code block remnants (```lang markers)
```

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

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Problems found (`--check`) or error |

## License

MIT
