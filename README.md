# fini

[![CI](https://github.com/tsukasaI/fini/actions/workflows/ci.yaml/badge.svg)](https://github.com/tsukasaI/fini/actions/workflows/ci.yaml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A file normalization CLI that fixes formatting deterministically (trailing
whitespace, EOF newline, line endings, invisible characters, ...) and reports
likely mistakes (TODO/FIXME, debug code, secret-looking values).

Run it as a pre-commit hook so every commit is normalized, whether a person or
an AI coding agent wrote it.

## Installation

### Cargo (from source)
```bash
cargo install --locked --git https://github.com/tsukasaI/fini --tag v0.7.0
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

To run fini on every commit, see Pre-commit hook below.

## Pre-commit hook (recommended)

Add to your `.pre-commit-config.yaml` (works with pre-commit and prek):

```yaml
repos:
  - repo: https://github.com/tsukasaI/fini
    rev: v0.7.0  # Use the latest release tag
    hooks:
      - id: fini        # Fixes formatting; the commit stops if a file changed, so re-stage and commit again
      - id: fini-check  # Fails on detections (TODO, FIXME, debug code, secrets), which fix mode only reports
```

Use both: fix mode exits 0 even when it reports detections (see Exit codes),
so `fini` alone never blocks a commit on a TODO or secret. `fini-check` does.

Both hooks use `language: system`: they run whichever `fini` is on your
`PATH`, and `rev` pins only the hook definition. Install fini first
(`cargo install --locked --git https://github.com/tsukasaI/fini --tag v0.7.0`,
or via Homebrew/Nix as above) and keep it updated.

## Usage

```bash
fini .                    # Fix current directory
fini src/main.rs          # Fix specific file
fini --check .            # Check only, exit 1 if problems
fini --diff .             # Preview changes
fini --quiet .            # Output only the names of changed or flagged files
fini -v .                 # Verbose: show all processed files
fini --init               # Generate fini.toml template
cat file.txt | fini --stdin  # Read from stdin, output to stdout
```

### Options

```
-c, --check             Check only (no modifications), exit 1 if problems found
-d, --diff              Show changes in diff format
-q, --quiet             Output only the names of files that were changed or
                        have problems
-v, --verbose           Show all processed files (including clean ones)
--stdin                 Read from stdin, output to stdout
--color                 Force colored output
--no-color              Disable colored output
--no-progress           Hide progress bar
--max-blank-lines <N>   Limit consecutive blank lines to N
--keep-zero-width       Keep ZWSP, word joiner and non-leading BOM (default: remove)
--keep-leading-blanks   Keep leading blank lines (default: remove)
--fix-code-blocks       Remove code block remnants (```lang markers)
--no-detect-todos       Skip TODO comment detection
--no-detect-fixmes      Skip FIXME comment detection
--no-detect-debug       Skip debug code detection
--strict-debug          Include console.error/eprintln in debug code detection
--no-detect-secrets     Skip secret pattern detection
--no-detect-bidi        Skip bidirectional control character detection
--max-line-length <N>   Maximum line length (warn if exceeded)
--max-file-size <BYTES> Maximum file size in bytes; larger files are skipped
                        like binary files (default: 10 MiB). Set a very
                        large value (e.g. the u64 max) to effectively
                        disable this gate
--exclude <PATTERN>     Exclude files matching glob pattern (repeatable)
--hidden                Include hidden files (dotfiles) in directory scans;
                        .git/ is still excluded either way
--init                  Generate fini.toml configuration template
--config <PATH>         Use specific config file
```

## Configuration

Create `fini.toml` in your project root (or run `fini --init`):

```toml
# Exclude files matching these patterns (gitignore-style globs)
# exclude = ["vendor/", "node_modules/", "*.min.js"]

[normalize]
max_blank_lines = 2          # Limit consecutive blank lines (default: no limit)
remove_zero_width = true     # Remove zero-width characters
remove_leading_blanks = true # Remove blank lines at the start of a file
fix_code_blocks = false      # Remove ``` markers
detect_todos = true          # Report TODO comments
detect_fixmes = true         # Report FIXME comments
detect_debug = true          # Report debug code (console.log, print, dbg!, ...)
strict_debug = false         # Also report console.error / eprintln!
detect_secrets = true        # Report secret patterns
detect_bidi = true           # Report bidirectional control characters
# max_line_length = 120      # Report longer lines (default: off)
max_file_size = 10485760     # Skip larger files, in bytes (default: 10 MiB)
```

The values shown are the defaults, except `max_blank_lines` and
`max_line_length`, which are off unless set.

Exclude patterns (`exclude` here, or `--exclude`) are matched relative to the
current directory, whatever the argument: with `src/gen/` excluded, `fini .`,
`fini src` and `fini src/gen/a.rs` all skip `src/gen/a.rs`. This includes
files passed by name, as pre-commit and editor hooks do, and a directory
argument that is itself excluded (`fini vendor` with `vendor/` excluded scans
nothing). Symlinked directories and `..` in an argument are resolved before
matching; a symlink as the last component is matched by its own location. If
a symlinked directory leads outside the current directory, the path is
matched as typed instead, so `data/x.csv` is still excluded by `data/`.

An argument outside the current directory is matched relative to itself
instead: patterns apply to what is found under a directory argument, and a
file passed by name is matched by its file name only (`*.snap` applies,
`vendor/` does not).

### Priority

CLI arguments > `fini.toml` > defaults

### Config Discovery

Searches upward from current directory, stops at git root. Outside a
git repository (no `.git` in any ancestor), only the current directory
is checked.

### .editorconfig

fini reads `.editorconfig` and warns if settings conflict with its fixed behaviors (always trims whitespace, always LF, always adds final newline). Only the `[*]` section is read; a per-type section such as `[*.md]` is not.

### Fixes that cannot be turned off

Trailing whitespace removal, full-width space (U+3000) conversion and LF line
endings apply to every file fini processes, and there is no per-file or
per-type switch for them. In some formats that changes meaning:

- Markdown: two trailing spaces are a hard line break
- TSV: a trailing tab is an empty last column
- Japanese prose: U+3000 is used for indentation and between names

Keep such files out with `exclude` (or `--exclude`), which also applies to
files passed by name.

## Features

| Rule | Description | Default |
|------|-------------|---------|
| EOF newline | Add `\n` if missing, normalize multiple trailing newlines | On |
| Line endings | CRLF/CR to LF | On |
| Trailing whitespace | Remove trailing spaces and tabs | On |
| Full-width spaces | Fix U+3000 to regular space (with warning) | On |
| Leading blank lines | Remove blank lines at file start | On |
| Zero-width characters | Remove ZWSP (U+200B), word joiner (U+2060) and a BOM past the file start. ZWJ, ZWNJ and directional marks are kept: emoji sequences and some scripts need them | On |
| Consecutive blank lines | Limit to N blank lines (`--max-blank-lines`) | Off |
| Code block remnants | Remove ``` markers (`--fix-code-blocks`) | Off |

Files are written atomically (write to a temp file in the same directory,
then rename over the original), which breaks any other hard link to the
file - other links keep the pre-fix content, since rename only repoints the
one directory entry fini wrote to. For the same reason the rewritten file
keeps its permission bits but not its owner (it becomes the user running
fini) or its extended attributes.

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

Kind identifiers: `todo`, `fixme`, `debug`, `secret`, `line-length`, `bidi`

The kinds are a comma-separated list of names. After the list (or right after
the directive, for "all kinds") only three things may follow: nothing, the
comment's closing marker (`*/`, `-->`, `#}`, `*)`, `%>`), or a reason
introduced by `--` (`fini:ignore todo -- legacy`). With anything else after
it (`fini:ignore: todo`, `fini:ignore todo debug`, a reason without `--`) the
directive suppresses nothing.

Only detections can be suppressed. Fix-type problems (fullwidth spaces, zero-width characters, leading blank lines, consecutive blank lines, code block remnants) are never suppressed, since the content changes regardless. Unrecognized or fix-type kinds have no effect; a directive naming only fix-type kinds (e.g. `fini:ignore fullwidth`) prints a one-time stderr hint per run, naming the first such file and how many more (not with `--quiet`). The hint only fires for files where fini finds at least one problem, because directives are not parsed in otherwise clean files.

If a line contains several directives, the first `fini:ignore` on it decides: a later `fini:ignore-next-line` on the same line is read as part of that first directive's kind list, not as its own directive. For example, `// TODO x fini:ignore fini:ignore-next-line` does not suppress the following line.

Everything on a line from `fini:ignore`/`fini:ignore-next-line` onward is
directive syntax, not code, so it's never scanned for TODO/FIXME markers or
counted as a detection.

Suppressions are counted in the run summary. Suppressed `secret` and `bidi`
detections are always reported on stderr with `file:line` (even with
`--quiet`) so they remain auditable.

## Secret detection and output

Secret detections are reported hint-only — the matched value is never printed.
`--diff` output masks lines matching a secret pattern
(`[line masked: potential …]`), and a private key is masked from its BEGIN
line through its END line, or to the end of the file if there is none. A
secret that no pattern matches will appear
verbatim in a diff: be careful publishing CI logs that include `--diff` output.

What is detected:

- PEM-style private key headers (`BEGIN ... PRIVATE KEY`) with any type label
  (none, `RSA`, `EC`, `DSA`, `OPENSSH`, `ENCRYPTED`, ...) and PGP private key
  blocks (`BEGIN PGP PRIVATE KEY BLOCK`)
- AWS access key ids (`AKIA…`, `ASIA…`) and quoted AWS secret access keys
- A quoted value assigned to `password`, `passwd`, `secret_key`, `api_key`,
  `auth_token` or `access_token`, with the key name quoted or not
  (`password = "…"`, `"api_key": "…"`)
- The same names in the dotenv shape: an upper-case variable, `=` with no
  spaces, an unquoted value (`API_KEY=…`, `export DB_PASSWORD=…`). The key
  word must end the name, so `PASSWORD_RESET_URL` and `API_KEY_FILE` are not
  reported
- Bearer tokens, GitHub tokens (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`,
  `github_pat_`), Slack tokens (`xoxb-`, `xoxp-`, `xoxa-`, `xoxr-`, `xoxs-`) and Stripe keys (`sk_live_`, `sk_test_`)

Not detected: other unquoted values (`password: hunter2` in YAML,
`password = value` in code), since those shapes are mostly not secrets.
Placeholder values assigned to those names are not reported either: a value
containing `your`, `xxx`, `changeme`, `example`, `placeholder` or `replace`, one
equal to the key name (`"password": "Password"`), a single repeated character
(`********`), or a reference such as `ENV.fetch(...)`. `--diff` still masks such
lines. Detection is a hint, not a guarantee.

fini is not a secret scanner, and its detection is best-effort. To gate
commits or CI on secrets, use a dedicated scanner such as gitleaks alongside
fini.

A `TODO` or `FIXME` marker directly preceded by a letter, digit or `_` is part
of a longer word (`AUTODO:`, `MY_TODO:`) and is not reported.

Disabling secret detection via `fini.toml` (`detect_secrets = false`) prints a
warning to stderr that `--quiet` does not suppress.

Directory scans skip hidden files by default (see Skipped below), including
`.env` and `.github/workflows/*.yml`, two of the most common places a secret
ends up. `fini --check .` as a CI secret-detection gate therefore does not
cover them unless you also pass `--hidden`, or check those paths directly
(`fini --check .env`). The GitHub Action below doesn't expose `--hidden` as
an input yet; pass hidden paths explicitly via `files` instead
(`files: '. .github'` - fini exits 2 on a path that doesn't exist, so only
list paths you know are present, e.g. add `.env` only if it's tracked in
your repo).

## Bidirectional control characters

A line holding a bidirectional embedding, override or isolate control
(U+202A to U+202E, U+2066 to U+2069) is reported as a detection: these
characters can make code display in a different order than it runs ("Trojan
Source"). fini reports them and never removes them, because right-to-left
text can use them legitimately.

Suppress one line with `fini:ignore bidi`, or turn the detection off with
`--no-detect-bidi` or `detect_bidi = false` in `fini.toml`. Turning it off
from `fini.toml` prints a warning to stderr that `--quiet` does not suppress.

## Skipped

- Binary files (null bytes in first 8KB)
- Files larger than `--max-file-size` (default: 10 MiB)
- UTF-16 and other non-UTF-8 text files (unsupported encodings)
- Symlinks (never followed or rewritten)
- Empty files
- Hidden files (`.foo`); pass `--hidden` to include them (`.git/` is still
  excluded either way). `--hidden` widens fini's own default, not your
  `.gitignore`/global excludes (inside a git repository - see the
  `.gitignore` entry below): a file your own ignore rules already hide
  (many developers' global gitignore lists `.env`) stays hidden regardless
- `.git/` directory
- `.gitignore` patterns - only within a git repository (a `.git` directory
  or file in, or anywhere above, the scanned path). Outside one (a
  `git archive` export, a tarball extraction, a directory that was never
  `git init`'d), `.gitignore` files are not consulted at all, and everything
  they'd otherwise exclude is scanned
- `.ignore` files (ripgrep-style) - honored everywhere, including outside a
  git repository. One exception: a file git already tracks is scanned even
  if a `.ignore` file lists it

Skipped binary / non-UTF-8 / symlink files are counted in the run summary;
use `--verbose` to list them individually with the skip reason. Arguments
skipped by an exclude pattern are counted there too (`N arguments excluded`)
and listed by `--verbose`.

`--stdin` applies the same size, UTF-16, binary and non-UTF-8 gates to its
input, honoring `--max-file-size` (and `max_file_size` in `fini.toml`). Since
there is no file to skip, such input exits 2 with the reason on stderr and
nothing on stdout. Empty input is still accepted.

## Claude Code Integration (optional)

The pre-commit hook above is the recommended setup. To also fix files as
Claude Code edits them, add to `.claude/settings.json` (requires [`jq`](https://jqlang.org/)):

```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Edit|Write",
      "hooks": [{
        "type": "command",
        "command": "jq -r '.tool_input.file_path // empty' | xargs -I{} fini {}"
      }]
    }]
  }
}
```

Claude Code passes the hook its input as JSON on stdin; `jq` extracts the
edited file's path. `xargs -I{}` passes that path as one argument, so paths
with spaces work, and runs nothing when the input has no `file_path`.

This hook starts fini after every Edit and Write. Over 30 days of Claude Code
sessions measured for this project, about 1 in 70 Write calls and almost no
Edit calls left anything to fix, and the pre-commit hook catches the same
problems at commit time. Because the hook rewrites the file right after the
agent's edit, the agent may need to re-read the file before its next edit to it.

## GitHub Action

```yaml
- uses: tsukasaI/fini@v0.7.0
```

Pin to a release tag as above, or to a full commit SHA for supply-chain hardening.

The action runs `fini --check` by default. Note that fix mode (without `--check`)
never fails on detection-only problems (TODOs, debug code, secrets) — always use
`--check` when gating CI.

### Options

```yaml
- uses: tsukasaI/fini@v0.7.0
  with:
    files: 'src/ tests/'         # Files/directories to check (default: .)
    check: 'true'                # Check mode, fail if issues found (default: true)
    version: 'v0.7.0'            # Specific version (default: latest)
    verify-attestation: 'true'   # Verify SLSA build provenance (default: true)
```

Set `verify-attestation` to `'false'` only if you pin `version` to a release
before v0.4.0, which predates attestations.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success (no problems / fix completed) |
| 1 | Problems found (`--check`) |
| 2 | Runtime error (I/O failure, invalid config or exclude pattern; also `--stdin` input that would be skipped: oversize, binary, UTF-16, or non-UTF-8) |

Fix mode (without `--check`) exits 0 even when detection-only problems
(TODOs, debug code, secrets) are reported — they are informational there.
Use `fini --check` as the CI gate.

On Unix, if fini's output pipe closes early (e.g. `fini --quiet dir | head`),
the process is killed by SIGPIPE rather than exiting with a code of its own
(the shell reports 141). In fix mode this stops processing at that point;
files not yet reached are left unprocessed.

## Development

```bash
cargo build                                  # Debug build
cargo test                                   # Unit and integration tests
cargo fmt --check                            # Formatting
cargo clippy --all-targets -- -D warnings    # Lints, as CI runs them
```

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
