# fini

[![CI](https://github.com/tsukasaI/fini/actions/workflows/ci.yml/badge.svg)](https://github.com/tsukasaI/fini/actions/workflows/ci.yml)
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

## Features

| Rule | Description |
|------|-------------|
| EOF newline | Add `\n` if missing, normalize multiple trailing newlines |
| Line endings | CRLF/CR to LF |
| Trailing whitespace | Remove trailing spaces and tabs |
| Full-width spaces | Fix U+3000 to regular space (with warning) |

## Skipped

- Binary files (null bytes in first 8KB)
- Empty files
- Hidden files (`.foo`)
- `.git/` directory
- `.gitignore` patterns

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Problems found (`--check`) or error |

## License

MIT
