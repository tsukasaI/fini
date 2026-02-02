# fini

A lightweight file normalization CLI tool for AI coding agents.

Standardizes file formatting as a finishing step after code editing.

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary at ./target/release/fini
```

## Usage

```bash
# Fix files in current directory
fini .

# Fix specific files
fini src/main.rs src/lib.rs

# Check only (no modifications), exit 1 if problems found
fini --check .

# Preview changes in diff format
fini --diff .

# Output only modified file names
fini --quiet .
```

## What it does

| Rule | Description |
|------|-------------|
| EOF newline | Add `\n` at end if missing, normalize multiple trailing newlines to one |
| Line endings | Convert CRLF (`\r\n`) and CR (`\r`) to LF (`\n`) |
| Trailing whitespace | Remove trailing spaces and tabs from each line |
| Full-width spaces | Detect and fix full-width space (U+3000) to regular space |

## Skipped files

- Binary files (detected by null bytes in first 8KB)
- Empty files
- Hidden files/directories (starting with `.`)
- `.git/` directory
- Files matching `.gitignore` patterns

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success / No problems found |
| 1 | Problems detected (`--check` mode) or error occurred |

## Output examples

Normal mode:
```
Warning: src/utils.rs:42 full-width space
Fixed: src/main.rs
Fixed: src/utils.rs

2 files fixed, 1 warnings
```

Check mode (`--check`):
```
Error: src/main.rs
  - missing EOF newline
  - trailing whitespace at line 15

1 files with problems
```

Diff mode (`--diff`):
```
--- src/main.rs
+++ src/main.rs
-    let x = 1;
+    let x = 1;
```

## License

MIT
