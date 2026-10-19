use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use tempfile::TempDir;

fn fini_cmd() -> Command {
    // Not the inherited cwd: cargo runs tests from the package root, which is a git
    // repo, so a developer's fini.toml there would leak into every test via the
    // upward config search. An empty dir outside any repo has nothing to find.
    //
    // The shared default cwd is writable, so a test running a mutating mode (e.g.
    // `--init`) without its own `.current_dir(...)` would write into it and leak into
    // every other test. Such tests must always set `.current_dir` to their own dir.
    //
    // A static is never dropped, so this dir is left behind after the run; the
    // `fini-it-cwd-` prefix makes those leftovers easy to recognize and sweep.
    static HERMETIC_CWD: OnceLock<TempDir> = OnceLock::new();
    let cwd = HERMETIC_CWD.get_or_init(|| {
        let dir = tempfile::Builder::new()
            .prefix("fini-it-cwd-")
            .tempdir()
            .unwrap();
        // Hermeticity silently fails if TMPDIR sits inside a git repo (fini.toml is
        // searched upward to the git root) or under a directory with an .editorconfig
        // (searched upward to `/`). Fail loudly instead, as the unit tests do.
        for ancestor in dir.path().ancestors() {
            for marker in [".git", ".editorconfig"] {
                assert!(
                    !ancestor.join(marker).exists(),
                    "integration tests need a hermetic cwd, but {} contains `{marker}` \
                     (an ancestor of the temp dir {}). Point TMPDIR at a directory \
                     outside any git repo / .editorconfig tree.",
                    ancestor.display(),
                    dir.path().display(),
                );
            }
        }
        dir
    });
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fini"));
    cmd.current_dir(cwd.path());
    cmd
}

// ===========================================
// Phase 3: CLI Integration Tests
// ===========================================

#[test]
fn test_version_output_shape_is_stable() {
    let output = fini_cmd().arg("--version").output().unwrap();
    assert_eq!(output.status.code(), Some(0));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .trim()
        .strip_prefix("fini ")
        .unwrap_or_else(|| panic!("version output must start with 'fini ': {stdout}"));
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(parts.len(), 3, "version must be X.Y.Z: {version}");
    for part in parts {
        part.parse::<u32>()
            .unwrap_or_else(|_| panic!("non-numeric version component {part:?}: {version}"));
    }
}

#[test]
fn test_check_mode_no_modification() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello").unwrap(); // Missing EOF newline

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // File should not be modified
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello");

    // Should exit with 1 (problems found)
    assert!(!output.status.success());
}

#[test]
fn test_check_mode_exit_code_0_when_no_problems() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap(); // Already normalized

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 0 (no problems)
    assert!(output.status.success());
}

#[test]
fn test_quiet_mode_output() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello").unwrap();

    let output = fini_cmd()
        .arg("--quiet")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should only output the filename
    assert!(stdout.contains("test.txt"));
    assert!(!stdout.contains("Fixed:"));
}

#[test]
fn test_normal_mode_fixes_files() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    // File should be fixed
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");

    // Should succeed
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fixed:"));
}

#[test]
fn test_diff_mode_shows_changes() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello").unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should show diff format
    assert!(stdout.contains("---"));
    assert!(stdout.contains("+++"));
}

#[test]
fn test_skip_binary_files() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    fs::write(&file, b"hello\x00world").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    // Binary file should not be modified
    assert_eq!(fs::read(&file).unwrap(), b"hello\x00world");

    // Should succeed (no files to fix is not an error)
    assert!(output.status.success());
}

#[test]
fn test_fix_trailing_whitespace() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello   \nworld\t\n").unwrap();

    fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\nworld\n");
}

#[test]
fn test_fix_crlf_line_endings() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\r\nline2\r\n").unwrap();

    fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "line1\nline2\n");
}

#[test]
fn test_fix_fullwidth_space() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\u{3000}world\n").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert_eq!(fs::read_to_string(&file).unwrap(), "hello world\n");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Warning:"));
    assert!(stdout.contains("full-width space"));
}

#[test]
fn test_multiple_files() {
    let dir = TempDir::new().unwrap();
    let file1 = dir.path().join("file1.txt");
    let file2 = dir.path().join("file2.txt");
    fs::write(&file1, "hello").unwrap();
    fs::write(&file2, "world").unwrap();

    fini_cmd()
        .arg(file1.to_str().unwrap())
        .arg(file2.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(fs::read_to_string(&file1).unwrap(), "hello\n");
    assert_eq!(fs::read_to_string(&file2).unwrap(), "world\n");
}

#[test]
fn test_many_files_preserve_order_and_identity_across_chunks() {
    // Regression test for the chunked-processing refactor in `run()`: files are
    // now processed in fixed-size batches (CHUNK_SIZE in src/lib.rs) instead of
    // all at once. Two things a batching bug could break:
    //   1. pairing — one file's fix landing in another file (chunk-boundary mixup)
    //   2. ordering — output no longer matches walk order across chunk boundaries
    // Use more files than CHUNK_SIZE so the run spans multiple chunks. Passing
    // them as explicit CLI args (rather than walking a directory) pins the input
    // order to something this test controls and can assert against — directory
    // walk order is filesystem-dependent and not something a test should assume.
    let dir = TempDir::new().unwrap();
    let file_count = 600;
    let mut cmd = fini_cmd();
    cmd.arg("--quiet");
    for i in 0..file_count {
        let path = dir.path().join(format!("file{i:04}.txt"));
        // Missing EOF newline (needs a fix) so every file is printed and written.
        fs::write(&path, format!("content-{i}")).unwrap();
        cmd.arg(&path);
    }

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    for i in 0..file_count {
        let content = fs::read_to_string(dir.path().join(format!("file{i:04}.txt"))).unwrap();
        assert_eq!(
            content,
            format!("content-{i}\n"),
            "mismatch for file{i:04}.txt"
        );
    }

    // --quiet prints exactly one line (the path) per fixed file, in apply order.
    // Extract each file's index from its printed path and confirm the sequence
    // is exactly 0..file_count — not just "no crash", but "order preserved".
    let stdout = String::from_utf8_lossy(&output.stdout);
    let indices: Vec<usize> = stdout
        .lines()
        .map(|line| {
            let name = Path::new(line.trim())
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap();
            name.strip_prefix("file").unwrap().parse().unwrap()
        })
        .collect();
    let expected: Vec<usize> = (0..file_count).collect();
    assert_eq!(
        indices, expected,
        "output order must match input walk order"
    );
}

#[test]
fn test_directory_recursive() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("file1.txt"), "hello").unwrap();
    fs::create_dir(dir.path().join("subdir")).unwrap();
    fs::write(dir.path().join("subdir/file2.txt"), "world").unwrap();

    fini_cmd()
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(
        fs::read_to_string(dir.path().join("file1.txt")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("subdir/file2.txt")).unwrap(),
        "world\n"
    );
}

// ===========================================
// Phase 2: Configuration File Tests
// ===========================================

#[test]
fn test_init_creates_config_file() {
    let dir = TempDir::new().unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg("--init")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "Created fini.toml"
    );

    let config_path = dir.path().join("fini.toml");
    assert!(config_path.exists());

    let content = fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[normalize]"));
    assert!(content.contains("max_blank_lines"));
}

#[test]
fn test_init_fails_if_config_exists() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("fini.toml");
    fs::write(&config_path, "existing").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg("--init")
        .output()
        .unwrap();

    // issue #84: --init failure is a runtime error (exit 2), not exit 1.
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn test_config_file_enables_fix_code_blocks() {
    let dir = TempDir::new().unwrap();

    // Create config file with fix_code_blocks enabled
    let config_path = dir.path().join("fini.toml");
    fs::write(
        &config_path,
        r#"
[normalize]
fix_code_blocks = true
"#,
    )
    .unwrap();

    // Create file with code blocks
    let file = dir.path().join("test.txt");
    fs::write(&file, "```rust\nfn main() {}\n```\n").unwrap();

    fini_cmd()
        .current_dir(dir.path())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Code blocks should be removed
    assert_eq!(fs::read_to_string(&file).unwrap(), "fn main() {}\n");
}

#[test]
fn test_cli_overrides_config_file() {
    let dir = TempDir::new().unwrap();

    // Create config file with remove_zero_width = false
    let config_path = dir.path().join("fini.toml");
    fs::write(
        &config_path,
        r#"
[normalize]
remove_zero_width = false
"#,
    )
    .unwrap();

    // Create file with zero-width character
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\u{200B}world\n").unwrap();

    // Run without CLI override - config should keep zero-width
    fini_cmd()
        .current_dir(dir.path())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Zero-width should NOT be removed (config says false)
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\u{200B}world\n");
}

#[test]
fn test_explicit_config_path() {
    let dir = TempDir::new().unwrap();

    // Create custom config file in subdirectory
    let config_dir = dir.path().join("config");
    fs::create_dir(&config_dir).unwrap();
    let config_path = config_dir.join("custom.toml");
    fs::write(
        &config_path,
        r#"
[normalize]
fix_code_blocks = true
"#,
    )
    .unwrap();

    // Create file with code blocks
    let file = dir.path().join("test.txt");
    fs::write(&file, "```rust\ncode\n```\n").unwrap();

    fini_cmd()
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Code blocks should be removed
    assert_eq!(fs::read_to_string(&file).unwrap(), "code\n");
}

#[test]
fn test_config_max_blank_lines() {
    let dir = TempDir::new().unwrap();

    // Create config file with max_blank_lines = 1
    let config_path = dir.path().join("fini.toml");
    fs::write(
        &config_path,
        r#"
[normalize]
max_blank_lines = 1
"#,
    )
    .unwrap();

    // Create file with multiple blank lines
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\n\n\n\nline2\n").unwrap();

    fini_cmd()
        .current_dir(dir.path())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should limit to 1 blank line
    assert_eq!(fs::read_to_string(&file).unwrap(), "line1\n\nline2\n");
}

#[test]
fn test_fix_then_check_idempotent_with_blank_limit_and_code_blocks() {
    // Regression test for issue #33: max_blank_lines + fix_code_blocks must
    // converge in a single `fini` fix pass, so an immediate `fini --check`
    // succeeds without needing to run fix twice.
    let dir = TempDir::new().unwrap();

    let config_path = dir.path().join("fini.toml");
    fs::write(
        &config_path,
        r#"
[normalize]
max_blank_lines = 1
fix_code_blocks = true
"#,
    )
    .unwrap();

    let file = dir.path().join("sample.md");
    fs::write(&file, "a\n\n```py\n\nb\n").unwrap();

    let fix_output = fini_cmd()
        .current_dir(dir.path())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(fix_output.status.success());

    let check_output = fini_cmd()
        .current_dir(dir.path())
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(
        check_output.status.success(),
        "check failed after fix: {}",
        String::from_utf8_lossy(&check_output.stdout)
    );
}

#[test]
fn test_invalid_config_toml_exits_2() {
    // A syntactically invalid fini.toml must be a hard failure (exit 2), not
    // a warning that silently falls back to defaults.
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("fini.toml");
    fs::write(&config_path, "invalid toml {{{\n").unwrap();

    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error"), "stderr: {stderr}");
}

#[test]
fn test_config_unknown_key_exits_2() {
    // A typo'd key (e.g. max_blank_line instead of max_blank_lines) must be
    // rejected outright, not silently ignored while the rest of the config
    // (or defaults) apply.
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("fini.toml");
    fs::write(&config_path, "[normalize]\nmax_blank_line = 2\n").unwrap();

    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error"), "stderr: {stderr}");
}

#[test]
fn test_explicit_config_path_missing_exits_2() {
    // An explicit --config pointing at a nonexistent file is also a hard
    // failure, not a silent fallback to defaults.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = fini_cmd()
        .arg("--config")
        .arg(dir.path().join("does-not-exist.toml").to_str().unwrap())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error"), "stderr: {stderr}");
}

// ===========================================
// Phase 3: Human Error Prevention Tests
// ===========================================

#[test]
fn test_cli_detects_todo_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "// TODO: fix this later\nfn main() {}\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 1 (problems found)
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TODO"));
}

#[test]
fn test_cli_detects_debug_code_in_check_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.js");
    fs::write(&file, "console.log('debug');\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 1 (problems found)
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("console.log"));
}

#[test]
fn test_cli_detects_secret_pattern() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.py");
    fs::write(&file, "API_KEY = \"sk_live_abcd12345678\"\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 1 (problems found)
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("secret"));
}

#[test]
fn test_cli_detects_long_lines() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, format!("{}\n", "a".repeat(150))).unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--max-line-length")
        .arg("120")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 1 (problems found)
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("too long"));
}

#[test]
fn test_cli_disable_todo_detection() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "// TODO: fix this later\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--no-detect-todos")
        .arg("--no-detect-debug")
        .arg("--no-detect-secrets")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 0 (TODO not flagged)
    assert!(output.status.success());
}

#[test]
fn test_cli_strict_debug_includes_console_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.js");
    fs::write(&file, "console.error('error');\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--strict-debug")
        .arg("--no-detect-todos")
        .arg("--no-detect-secrets")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 1 (console.error flagged in strict mode)
    assert!(!output.status.success());
}

#[test]
fn test_cli_default_excludes_console_error() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.js");
    fs::write(&file, "console.error('error');\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--no-detect-todos")
        .arg("--no-detect-secrets")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 0 (console.error not flagged by default)
    assert!(output.status.success());
}

#[test]
fn test_config_file_controls_detections() {
    let dir = TempDir::new().unwrap();

    // Create config file with detect_todos = false
    let config_path = dir.path().join("fini.toml");
    fs::write(
        &config_path,
        r#"
[normalize]
detect_todos = false
detect_debug = false
detect_secrets = false
"#,
    )
    .unwrap();

    // Create file with TODO
    let file = dir.path().join("test.rs");
    fs::write(&file, "// TODO: fix this\n").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should exit with 0 (TODO not flagged per config)
    assert!(output.status.success());
}

// ===========================================
// Exit Codes (issue #34) & Invalid Exclude (issue #32)
// ===========================================

#[test]
fn test_invalid_exclude_pattern_fails_closed() {
    // Regression test for issue #32: an invalid --exclude glob must not be
    // silently swallowed. It should abort with a non-zero exit and an stderr
    // message, and must not process (and silently "pass") any files.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("f.md");
    fs::write(&file, "hello   \n").unwrap(); // trailing whitespace, should be fixed

    let output = fini_cmd()
        .arg("--exclude")
        .arg("[invalid")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid exclude pattern"),
        "stderr: {stderr}"
    );

    // File must be left untouched — no silent pass-through.
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello   \n");
}

#[test]
fn test_exit_code_0_on_success() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn test_exit_code_1_on_check_mode_problems() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello").unwrap(); // missing EOF newline

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn test_check_mode_reports_extra_trailing_newlines_with_crlf() {
    // Regression test: trailing-newline counting must not undercount CRLF
    // line endings (each "\r\n" is one newline, not zero).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "X\r\n\r\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("extra trailing newline(s) removed"),
        "expected extra trailing newline(s) removed message, got: {stdout}"
    );
}

#[test]
fn test_check_mode_reports_original_line_number_after_leading_blanks_removed() {
    // Regression test for issue #76: detection line numbers must refer to the
    // original file the user has open, not the post-fix content. `--check`
    // never writes, so a line number shifted by the (unapplied) leading-blank
    // fix is simply wrong.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("c.txt");
    fs::write(&file, "\n\n\n// TODO: fix this\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TODO comment at line 4"),
        "expected TODO reported at original line 4, got: {stdout}"
    );
}

#[test]
fn test_exit_code_2_on_invalid_exclude_pattern() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = fini_cmd()
        .arg("--exclude")
        .arg("[invalid")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn test_exit_code_2_on_write_permission_error() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("readonly.txt");
    fs::write(&file, "hello   \n").unwrap(); // trailing whitespace triggers a write

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&file, perms.clone()).unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    // Restore permissions so TempDir cleanup can remove the file.
    perms.set_mode(0o644);
    fs::set_permissions(&file, perms).unwrap();

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Specifically the mode-bit guard's own message, so removing that guard
    // (issue #100 kept it alongside the newer write-probe check, for root)
    // would be caught here, not just a generic "some write error" match.
    assert!(stderr.contains("read-only file"), "stderr: {stderr}");
}

// issue #100: write_atomic's read-only guard must check actual writability
// (can the current process open this file for write?), not mode bits alone
// (permissions().readonly(), i.e. "does the mode have any write bit set at
// all?"). Those two questions diverge for a file owned by another user with
// mode 0644: the write bit is set, but the current process still can't
// write it. That full scenario needs a second uid to reproduce for real.
// macOS's "user immutable" flag (settable by the file's own owner, no root
// needed) reproduces the mode-bits-say-writable half without one - but it
// also blocks the eventual rename, unlike the real cross-user case, so this
// only proves the new open-for-write probe is what rejects the file (see
// the stderr assertion below), not that it prevented a silent replacement.
#[cfg(target_os = "macos")]
#[test]
fn test_issue_100() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("locked.txt");
    fs::write(&file, "hello   \n").unwrap(); // trailing whitespace triggers a write

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file, perms).unwrap();
    assert!(Command::new("chflags")
        .arg("uchg")
        .arg(&file)
        .status()
        .unwrap()
        .success());
    // Clears the immutable flag on drop even if a later assertion panics,
    // so a failure here doesn't leave an undeletable file behind for
    // TempDir's own Drop to fail on.
    struct ClearImmutableOnDrop(std::path::PathBuf);
    impl Drop for ClearImmutableOnDrop {
        fn drop(&mut self) {
            let _ = Command::new("chflags").arg("nouchg").arg(&self.0).status();
        }
    }
    let _clear_on_drop = ClearImmutableOnDrop(file.clone());

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "mode bits alone (0644) must not be trusted as proof of writability"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Specifically the new open-for-write probe's own message, not just
    // "some write error occurred" - chflags uchg also blocks the eventual
    // rename, so a generic io-error assertion here would pass even with
    // the fix reverted (persist() would still fail, just later and with a
    // different message).
    assert!(
        stderr.contains("cannot open for write"),
        "the write-probe guard, not just a later rename failure, must be what rejects this: {stderr}"
    );
}

// ===========================================
// Phase 5: Exclude Patterns
// ===========================================

#[test]
fn test_exclude_pattern_skips_matching_files() {
    let dir = TempDir::new().unwrap();
    let file1 = dir.path().join("keep.txt");
    let file2 = dir.path().join("skip.min.js");
    fs::write(&file1, "hello").unwrap();
    fs::write(&file2, "world").unwrap();

    fini_cmd()
        .arg("--exclude")
        .arg("*.min.js")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(fs::read_to_string(&file1).unwrap(), "hello\n");
    assert_eq!(fs::read_to_string(&file2).unwrap(), "world"); // unchanged
}

#[test]
fn test_exclude_directory_pattern() {
    let dir = TempDir::new().unwrap();
    let vendor_dir = dir.path().join("vendor");
    fs::create_dir(&vendor_dir).unwrap();
    let file1 = dir.path().join("main.txt");
    let file2 = vendor_dir.join("lib.txt");
    fs::write(&file1, "hello").unwrap();
    fs::write(&file2, "world").unwrap();

    fini_cmd()
        .arg("--exclude")
        .arg("vendor/")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(fs::read_to_string(&file1).unwrap(), "hello\n");
    assert_eq!(fs::read_to_string(&file2).unwrap(), "world"); // unchanged
}

#[test]
fn test_config_exclude_patterns() {
    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("fini.toml");
    fs::write(
        &config_path,
        r#"
exclude = ["*.log"]
"#,
    )
    .unwrap();

    let file1 = dir.path().join("main.txt");
    let file2 = dir.path().join("debug.log");
    fs::write(&file1, "hello").unwrap();
    fs::write(&file2, "world").unwrap();

    fini_cmd()
        .current_dir(dir.path())
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(fs::read_to_string(&file1).unwrap(), "hello\n");
    assert_eq!(fs::read_to_string(&file2).unwrap(), "world"); // unchanged
}

// ===========================================
// Inline Ignore Directives
// ===========================================

#[test]
fn test_ignore_inline_todo_check_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "// TODO: fix this fini:ignore\nfn main() {}\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Ignored TODO should not cause failure
    assert!(output.status.success());
}

#[test]
fn test_ignore_next_line_check_mode() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(
        &file,
        "// fini:ignore-next-line\n// TODO: fix this\nfn main() {}\n",
    )
    .unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
}

#[test]
fn test_ignore_selective_suppresses_only_specified() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.js");
    // debug is ignored but TODO is not
    fs::write(&file, "console.log('x'); // TODO: fix fini:ignore debug\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Should still fail because TODO is not ignored
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TODO"));
}

#[test]
fn test_ignore_in_fix_mode_suppresses_warning() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.rs");
    fs::write(&file, "// TODO: intentional fini:ignore\n").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should not mention TODO in output
    assert!(!stdout.contains("TODO"));
}

// ===========================================
// Diff Output & Secret Masking (issues #38, #44)
// ===========================================

#[test]
fn test_diff_masks_secret_lines() {
    // Regression test for issue #44: --diff must not print raw secret values,
    // including unchanged context lines near a change.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.env");
    fs::write(&file, "password = \"supersecret123\"\ncode here   \n").unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("supersecret123"),
        "raw secret leaked into diff output: {stdout}"
    );
    assert!(
        stdout.contains("[line masked: potential hardcoded secret]"),
        "masked placeholder missing: {stdout}"
    );
}

#[test]
fn test_diff_masks_secret_split_by_zero_width_in_file_mode() {
    // issue #156: a zero-width char inside the value must not leave the line unmasked.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.env");
    fs::write(
        &file,
        "password = \"hunter\u{200B}2hunter2\"\ncode here   \n",
    )
    .unwrap();

    for keep in [false, true] {
        let mut cmd = fini_cmd();
        cmd.arg("--diff");
        if keep {
            cmd.arg("--keep-zero-width");
        }
        let output = cmd.arg(file.to_str().unwrap()).output().unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for text in [&stdout, &stderr] {
            assert!(
                !text.contains("hunter2hunter2") && !text.contains("hunter\u{200B}2hunter2"),
                "raw secret leaked (keep_zero_width={keep}): {text}"
            );
        }
        assert!(
            stdout.contains("[line masked: potential hardcoded secret]"),
            "masked placeholder missing (keep_zero_width={keep}): {stdout}"
        );
    }
}

#[test]
fn test_check_diff_masks_secret_split_by_zero_width_in_file_mode() {
    // issue #156, --check variant of the file-mode test above.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.env");
    fs::write(
        &file,
        "password = \"hunter\u{200B}2hunter2\"\ncode here   \n",
    )
    .unwrap();

    for keep in [false, true] {
        let mut cmd = fini_cmd();
        cmd.arg("--check").arg("--diff");
        if keep {
            cmd.arg("--keep-zero-width");
        }
        let output = cmd.arg(file.to_str().unwrap()).output().unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for text in [&stdout, &stderr] {
            assert!(
                !text.contains("hunter2hunter2") && !text.contains("hunter\u{200B}2hunter2"),
                "raw secret leaked (keep_zero_width={keep}): {text}"
            );
        }
        assert!(
            stdout.contains("[line masked: potential hardcoded secret]"),
            "masked placeholder missing (keep_zero_width={keep}): {stdout}"
        );
    }
}

#[test]
fn test_diff_masks_secret_split_by_other_invisible_chars() {
    // issue #179: invisible code points beyond the fixer's zero-width set.
    let dir = TempDir::new().unwrap();
    for invisible in ['\u{00AD}', '\u{2062}', '\u{FE0F}', '\u{2066}'] {
        let file = dir.path().join("test.env");
        let raw =
            format!("aws_access_key_id = \"AKIAABCD{invisible}EFGHIJKLMNOP\"\ncode here   \n");
        fs::write(&file, &raw).unwrap();

        for check in [false, true] {
            let mut cmd = fini_cmd();
            if check {
                cmd.arg("--check");
            }
            let output = cmd
                .arg("--diff")
                .arg(file.to_str().unwrap())
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            for text in [&stdout, &stderr] {
                assert!(
                    !text.contains("AKIAABCD") && !text.contains("EFGHIJKLMNOP"),
                    "raw secret leaked (U+{:04X}, check={check}): {text}",
                    invisible as u32
                );
            }
            assert!(
                stdout.contains("[line masked: potential"),
                "masked placeholder missing (U+{:04X}, check={check}): {stdout}",
                invisible as u32
            );
        }
    }
}

#[test]
fn test_check_keep_zero_width_reports_split_secret() {
    // issue #179: detection must see through invisibles like the diff mask does.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.env");
    fs::write(&file, "password = \"hunter\u{200B}2hunter2\"\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--keep-zero-width")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "split secret not reported: {stdout}"
    );
    assert!(stdout.contains("line 1"), "wrong line: {stdout}");
}

#[test]
fn test_stdin_check_diff_masks_secret_split_by_zero_width() {
    use std::io::Write;
    use std::process::Stdio;

    for keep in [false, true] {
        let mut cmd = fini_cmd();
        cmd.arg("--stdin").arg("--check").arg("--diff");
        if keep {
            cmd.arg("--keep-zero-width");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all("password = \"hunter\u{200B}2hunter2\"\nhello   \n".as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for text in [&stdout, &stderr] {
            assert!(
                !text.contains("hunter2hunter2") && !text.contains("hunter\u{200B}2hunter2"),
                "raw secret leaked (keep_zero_width={keep}): {text}"
            );
        }
        assert!(
            stderr.contains("[line masked: potential hardcoded secret]"),
            "masked placeholder missing (keep_zero_width={keep}): {stderr}"
        );
    }
}

#[test]
fn test_check_diff_shows_detection_only_problems() {
    // Regression test: a file whose only problems are detection-only
    // (secret pattern here; same applies to TODO/FIXME/debug/long-line)
    // never changes result.content vs original, so `--check --diff` used to
    // print an empty `---`/`+++` diff and return before ever reaching the
    // problem list, silently dropping the secret hint (exit 1 with no
    // indication why). The problem list must still print even when there's
    // no content diff to show.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("secret.py");
    fs::write(&file, "password = \"supersecret123\"\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "secret hint missing from --check --diff output: {stdout}"
    );
}

#[test]
fn test_stdin_check_ignore_fullwidth_still_reports_line_1() {
    // Issue #165: fix-type kinds are not suppressible, so --check must explain
    // why it fails instead of printing a bare `Error: stdin`.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all("a\u{3000}b # fini:ignore fullwidth\n".as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("full-width space at line 1"),
        "missing line-1 bullet: {stderr}"
    );
}

#[test]
fn test_fix_kind_only_ignore_hint_printed_once_across_files() {
    let dir = TempDir::new().unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(
            dir.path().join(name),
            "a\u{3000}b # fini:ignore fullwidth\n",
        )
        .unwrap();
    }

    let output = fini_cmd()
        .arg("--check")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr
            .matches("Hint: fini:ignore only suppresses detections")
            .count(),
        1,
        "hint must print exactly once: {stderr}"
    );
    let hint_line = stderr
        .lines()
        .find(|l| l.contains("Hint: fini:ignore"))
        .unwrap();
    assert!(hint_line.contains("cannot be suppressed"), "{hint_line}");
    assert!(
        hint_line.contains("a.txt") || hint_line.contains("b.txt"),
        "hint must name a file: {hint_line}"
    );
    assert!(hint_line.ends_with(" and 1 more)"), "{hint_line}");

    let quiet = fini_cmd()
        .arg("--check")
        .arg("--quiet")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("Hint:"));
}

#[test]
fn test_fix_kind_only_ignore_hint_names_single_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("only.txt");
    fs::write(&file, "a\u{3000}b # fini:ignore fullwidth\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    let hint_line = stderr
        .lines()
        .find(|l| l.contains("Hint: fini:ignore"))
        .unwrap();
    assert!(hint_line.contains("only.txt"), "{hint_line}");
    assert!(!hint_line.contains("more"), "{hint_line}");
}

#[test]
fn test_stdin_check_fix_kind_only_ignore_prints_hint_unless_quiet() {
    let input = "a\u{3000}b # fini:ignore fullwidth\n".as_bytes();

    let out = run_stdin(&["--check"], input);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Hint: fini:ignore only suppresses detections"),
        "{stderr}"
    );
    assert!(stderr.contains("cannot be suppressed"), "{stderr}");

    let quiet = run_stdin(&["--check", "--quiet"], input);
    assert_eq!(quiet.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&quiet.stderr).contains("Hint:"));
}

#[test]
fn test_ignore_with_detection_kind_prints_no_fix_kind_hint() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.txt");
    fs::write(&file, "a\u{3000}b # TODO fini:ignore todo\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Hint:"));
}

#[test]
fn test_stdin_check_diff_goes_to_stderr() {
    // Regression test for issue #38: --stdin --check --diff must keep stdout
    // clean (its contract is "normalized content only") and put the diff on
    // stderr.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .arg("--diff")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello   \n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        stderr.contains("--- stdin"),
        "diff missing on stderr: {stderr}"
    );
    assert!(
        stderr.contains("+++ stdin"),
        "diff missing on stderr: {stderr}"
    );
}

#[test]
fn test_diff_mode_previews_without_writing() {
    // Regression test: README documents `fini --diff .` as "Preview changes",
    // but bare --diff used to write the normalized content to disk anyway
    // (only --check --diff skipped the write). --diff alone must be a
    // read-only preview: print a diff, leave the file untouched, exit 0.
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello").unwrap(); // missing EOF newline

    let before_content = fs::read(&file).unwrap();
    let before_mtime = fs::metadata(&file).unwrap().modified().unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("---"), "missing diff header: {stdout}");
    assert!(stdout.contains("+++"), "missing diff header: {stdout}");

    let after_content = fs::read(&file).unwrap();
    let after_mtime = fs::metadata(&file).unwrap().modified().unwrap();
    assert_eq!(
        before_content, after_content,
        "--diff must not modify file content"
    );
    assert_eq!(
        before_mtime, after_mtime,
        "--diff must not touch the file (mtime changed)"
    );

    assert_eq!(output.status.code(), Some(0));
}

// ===========================================
// Secret Detection Trust Boundary & Audit Trail (issues #45, #46)
// ===========================================

#[test]
fn test_config_disabling_secret_detection_warns_even_in_quiet_mode() {
    // Regression test for issue #45: a repo-local fini.toml downgrading the
    // security posture must never be silent, and --quiet must not hide it.
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("fini.toml"),
        "[normalize]\ndetect_secrets = false\n",
    )
    .unwrap();
    let file = dir.path().join("clean.txt");
    fs::write(&file, "hello\n").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg("--quiet")
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("disables secret detection"),
        "missing security-posture warning: {stderr}"
    );
}

#[test]
fn test_suppressed_secret_leaves_audit_trail() {
    // Regression test for issue #46: `fini:ignore secret` still suppresses the
    // failure, but the suppression must be visible (stderr + summary).
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.py");
    fs::write(
        &file,
        "password = \"supersecret123\" # fini:ignore secret\n",
    )
    .unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0), "suppression must still work");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("suppressed by fini:ignore"),
        "missing stderr audit trail: {stderr}"
    );
    assert!(
        stderr.contains(":1"),
        "audit trail must include the line number"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("suppressed"),
        "summary must count suppressions: {stdout}"
    );
}

// ===========================================
// Skip Visibility & UTF-16 (issue #40)
// ===========================================

#[test]
fn test_utf16_file_skipped_with_accurate_reason() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("utf16.txt");
    // "TODO: fix\n" encoded as UTF-16LE with BOM
    let mut bytes = vec![0xFF, 0xFE];
    for unit in "TODO: fix\n".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&file, &bytes).unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--verbose")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // Not inspected, so no problems — but the skip must be visible and accurate
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("UTF-16"),
        "skip reason must say UTF-16: {stdout}"
    );
    assert_eq!(fs::read(&file).unwrap(), bytes, "file must be untouched");
}

#[test]
fn test_skipped_files_counted_in_summary_without_verbose() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("binary.bin");
    fs::write(&file, b"hello\x00world").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("1 files skipped"),
        "summary must surface skipped files: {stdout}"
    );
}

// An empty file is trivially done, not "not inspected": it must not appear in
// the skipped count (pins `SkipReason::counts_as_skipped` for Empty).
#[test]
fn test_empty_file_not_counted_as_skipped() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, b"").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("skipped"),
        "an empty file must not be reported as skipped: {stdout}"
    );
}

#[test]
fn test_colored_summary_wraps_each_part_and_keeps_secrets_suffix_inside() {
    // Integration tests run without a TTY, so colors are normally off and a
    // misplaced reset (or the " (N secrets)" suffix landing after it) would
    // pass every other test. Force colors and pin the exact escape layout.
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("fixable.txt"), "hello   \n").unwrap();
    fs::write(dir.path().join("binary.bin"), b"hello\x00world").unwrap();
    // Otherwise clean, so only the fixable file counts as fixed.
    fs::write(
        dir.path().join("keys.txt"),
        "password = \"supersecret123\" # fini:ignore secret\n",
    )
    .unwrap();

    let output = fini_cmd()
        .env_remove("NO_COLOR")
        .arg("--color")
        .arg("--no-progress")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "\x1b[32m1 files fixed\x1b[0m",
        "\x1b[36m1 files skipped (see --verbose)\x1b[0m",
        "\x1b[33m1 problems suppressed by fini:ignore (1 secrets)\x1b[0m",
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in summary: {stdout:?}"
        );
    }
}

// ===========================================
// Atomic Writes & Symlinks (issue #35)
// ===========================================

#[cfg(unix)]
#[test]
fn test_fix_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let file = dir.path().join("script.sh");
    fs::write(&file, "echo hi   \n").unwrap(); // trailing whitespace triggers a write

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&file, perms).unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();
    assert_eq!(output.status.code(), Some(0));

    assert_eq!(fs::read_to_string(&file).unwrap(), "echo hi\n");
    let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "atomic write must preserve permissions");
}

#[cfg(unix)]
#[test]
fn test_symlink_target_never_rewritten() {
    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, "hello   \n").unwrap(); // would be "fixed" if followed
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let output = fini_cmd().arg(link.to_str().unwrap()).output().unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "hello   \n",
        "symlink target must not be rewritten"
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink itself must survive"
    );
}

// ===========================================
// Overlapping Path Arguments (issue #81)
// ===========================================

#[test]
fn test_overlapping_dir_and_file_args_fix_once() {
    let dir = TempDir::new().unwrap();
    let subdir = dir.path().join("dupdir");
    fs::create_dir(&subdir).unwrap();
    let file = subdir.join("f.txt");
    fs::write(&file, "hello   \n").unwrap();

    let output = fini_cmd()
        .arg(subdir.to_str().unwrap())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(!stderr.contains("changed on disk"), "stderr: {stderr}");
    assert_eq!(
        stdout.matches("Fixed:").count(),
        1,
        "file named by two overlapping path args must be fixed exactly once: {stdout}"
    );
    assert!(
        stdout.contains("1 files fixed"),
        "summary must count the file once: {stdout}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn test_overlapping_dir_and_file_args_check_mode_counts_once() {
    let dir = TempDir::new().unwrap();
    let subdir = dir.path().join("dupdir");
    fs::create_dir(&subdir).unwrap();
    let file = subdir.join("f.txt");
    fs::write(&file, "hello   \n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(subdir.to_str().unwrap())
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "stdout: {stdout}");
    assert_eq!(
        stdout.matches("f.txt").count(),
        1,
        "file named by two overlapping path args must be reported exactly once: {stdout}"
    );
    assert!(
        stdout.contains("1 files with problems"),
        "summary must count the file once: {stdout}"
    );
    // File must be untouched in check mode.
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello   \n");
}

// ===========================================
// Stdin Mode
// ===========================================

#[test]
fn test_stdin_normalizes_content() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello   \nworld")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "hello\nworld\n");
}

/// Run `fini --stdin <args>` with `input`, returning the finished output.
/// The write result is ignored: the process may exit before reading it all.
fn run_stdin(args: &[&str], input: &[u8]) -> std::process::Output {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(input);
    drop(stdin);
    child.wait_with_output().unwrap()
}

#[test]
fn test_stdin_keeps_form_feed_and_nbsp_only_lines() {
    let out = run_stdin(&[], b"\x0c\nfoo\n");
    assert_eq!(out.stdout, b"\x0c\nfoo\n");

    let out = run_stdin(&[], "\u{a0}\nfoo\n".as_bytes());
    assert_eq!(out.stdout, "\u{a0}\nfoo\n".as_bytes());

    let out = run_stdin(&["--max-blank-lines", "1"], b"a\n\x0c\n\x0c\n\x0c\nb\n");
    assert_eq!(out.stdout, b"a\n\x0c\n\x0c\n\x0c\nb\n");
}

#[test]
fn test_stdin_fullwidth_only_leading_line_removed_idempotently() {
    let first = run_stdin(&[], "\u{3000}\nfoo\n".as_bytes());
    assert_eq!(first.stdout, b"foo\n");
    let second = run_stdin(&[], &first.stdout);
    assert_eq!(second.stdout, first.stdout);
}

#[test]
fn test_stdin_oversize_input_exits_2_without_output() {
    let output = run_stdin(&["--max-file-size", "16"], &[b'a'; 100_000]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("too large"), "stderr: {stderr}");
}

#[test]
fn test_stdin_input_at_size_limit_is_processed() {
    let output = run_stdin(&["--max-file-size", "6"], b"hello\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"hello\n");
}

#[test]
fn test_stdin_utf16_bom_exits_2_without_output() {
    for bom in [[0xFF, 0xFE], [0xFE, 0xFF]] {
        let mut input = bom.to_vec();
        input.extend_from_slice(b"h\0i\0\n\0");
        let output = run_stdin(&[], &input);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("UTF-16"), "stderr: {stderr}");
    }
}

#[test]
fn test_stdin_nul_bytes_exit_2_without_output() {
    let output = run_stdin(&[], b"hello\0world\n");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("binary"), "stderr: {stderr}");
}

#[test]
fn test_stdin_non_utf8_exits_2_without_output() {
    let output = run_stdin(&[], b"\xc3\x28 bad\n");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("non-UTF-8"), "stderr: {stderr}");
}

#[test]
fn test_stdin_nul_bytes_exit_2_in_check_mode() {
    let output = run_stdin(&["--check"], b"hello\0world\n");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn test_stdin_empty_input_still_succeeds() {
    let output = run_stdin(&[], b"");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
}

#[test]
fn test_stdin_check_mode_detects_problems() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"hello   ").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
}

#[test]
fn test_stdin_check_mode_passes_clean_input() {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();

    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
}

#[test]
fn test_stdin_check_detection_only_prints_problem_to_stderr() {
    // Regression test for issue #79: detection-only problems (no content
    // changes) used to exit 1 with zero diagnostics anywhere.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"TODO: x\n").unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        stderr.contains("TODO comment at line 1"),
        "problem diagnostic missing from stderr: {stderr}"
    );
}

#[test]
fn test_stdin_check_diff_no_content_change_omits_diff_header() {
    // Regression test for issue #79: --stdin --check --diff used to print an
    // empty `--- stdin` / `+++ stdin` header even when nothing but a
    // detection-only problem was found (no content diff to show).
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .arg("--diff")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"TODO: x\n").unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        !stderr.contains("--- stdin"),
        "diff header should be omitted with no content change: {stderr}"
    );
    assert!(
        stderr.contains("TODO comment at line 1"),
        "problem diagnostic missing from stderr: {stderr}"
    );
}

#[test]
fn test_stdin_check_diff_with_content_change_shows_diff_and_problem() {
    // Regression test for issue #79: when both a content change and a
    // detection-only problem are present, --diff must still show the diff
    // (no regression from the no-op-diff guard) and the problem list must
    // still run alongside it.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .arg("--diff")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"TODO: x  \n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        stderr.contains("--- stdin") && stderr.contains("+++ stdin"),
        "diff header missing on real content change: {stderr}"
    );
    assert!(
        stderr.contains("TODO comment at line 1"),
        "problem diagnostic missing from stderr: {stderr}"
    );
}

#[test]
fn test_stdin_check_fix_only_problem_prints_summary_without_diff() {
    // Regression test for issue #79: a fix-only problem (trailing whitespace,
    // missing EOF newline, ...) never populates result.problems, so without
    // this summary --stdin --check exited 1 with zero diagnostics for the
    // most common trigger.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"hello   \n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        stderr.contains("trailing whitespace at line 1"),
        "fix-only diagnostic missing from stderr: {stderr}"
    );
}

#[test]
fn test_stdin_check_crlf_only_change_prints_diagnostic() {
    // Regression test for issue #79: CRLF normalization is a fix-only
    // transform (no ProblemKind variant for it), so before issue #83's fix
    // it used to leave stderr completely empty despite the exit 1.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"hello\r\n").unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        stderr.contains("line endings normalized"),
        "stderr must explain the CRLF-only check failure: {stderr}"
    );
}

#[test]
fn test_stdin_check_diff_masked_to_no_diff_omits_diff_header() {
    // Regression test for issue #79: masking a secret line can make the
    // masked original and masked normalized content identical even though
    // result.has_changes() is true (the unmasked content did change, e.g.
    // trailing whitespace on the secret line itself) - the diff header must
    // still be omitted in that case, and the secret value must never reach
    // stderr.
    use std::io::Write;
    use std::process::Stdio;

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--check")
        .arg("--diff")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"aws_secret_access_key = \"AKIAABCDEFGHIJKLMNOP\"  \n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
    assert!(
        !stderr.contains("--- stdin"),
        "diff header should be omitted when masking collapses the diff: {stderr}"
    );
    assert!(
        !stderr.contains("AKIAABCDEFGHIJKLMNOP"),
        "secret value must never reach stderr: {stderr}"
    );
    assert!(
        stderr.contains("potential secret"),
        "secret hint missing from stderr: {stderr}"
    );
}

// issue #166: when masking collapses the whole diff, stdin must still print
// an `Error: stdin` header (as file mode does) so no problem bullet is ever
// printed bare.
#[test]
fn test_stdin_check_diff_masked_away_still_prints_error_header() {
    use std::io::Write;
    use std::process::Stdio;

    // Trailing whitespace, and a CRLF change, and a U+3000 on a secret line:
    // each leaves masked before/after identical.
    for (input, bullet) in [
        (
            &b"aws_secret_access_key = \"AKIAABCDEFGHIJKLMNOP\"  \n"[..],
            "  - trailing whitespace at line 1",
        ),
        (
            &b"aws_secret_access_key = \"AKIAABCDEFGHIJKLMNOP\"\r\n"[..],
            "  - CRLF/CR line endings normalized to LF",
        ),
        (
            "aws_secret_access_key = \"AKIAABCDEFGHIJKLMNOP\"\u{3000}\n".as_bytes(),
            "  - full-width space at line 1",
        ),
    ] {
        let mut child = fini_cmd()
            .arg("--stdin")
            .arg("--check")
            .arg("--diff")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        child.stdin.take().unwrap().write_all(input).unwrap();

        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(1));

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stdout.is_empty(), "stdout must stay clean: {stdout}");
        assert!(
            stderr.contains("Error: stdin"),
            "Error header missing from stderr: {stderr}"
        );
        let header_pos = stderr.find("Error: stdin");
        let bullet_pos = stderr.find(bullet);
        assert!(
            header_pos.is_some() && bullet_pos.is_some() && header_pos < bullet_pos,
            "Error header must precede the bullet {bullet:?}: {stderr}"
        );
        assert!(
            !stderr.contains("--- stdin"),
            "hunkless diff header must stay omitted: {stderr}"
        );
        assert!(
            !stderr.contains("AKIAABCDEFGHIJKLMNOP"),
            "secret value must never reach stderr: {stderr}"
        );
    }
}

// issue #78: fix mode (without --check) must not print "Fixed:" for a file
// that had no auto-fixable changes but does have detection-only problems
// (TODO/FIXME/debug/secret) — and must show those problems, not silently
// drop them.
#[test]
fn test_issue_78() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // Already normalized (no line-ending/whitespace/EOF fixes needed), but
    // contains a TODO marker, which is detection-only.
    fs::write(&file, "// TODO: fix this later\n").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();
    assert!(output.status.success());

    // File content must be unchanged.
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "// TODO: fix this later\n"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Fixed:"),
        "must not claim the file was fixed: {stdout}"
    );
    assert!(
        stdout.contains("Detected:"),
        "must report the detection-only file: {stdout}"
    );
    assert!(
        stdout.contains("TODO comment"),
        "must show the TODO detection instead of silently dropping it: {stdout}"
    );
}

// issue #78 (follow-up): a file that gets an auto-fix AND has a
// detection-only problem must show both "Fixed:" and the detection — not
// just the fix.
#[test]
fn test_issue_78_mixed_fix_and_detection() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("mixed.txt");
    // Trailing whitespace triggers an auto-fix; the TODO is detection-only.
    fs::write(&file, "some code   \n// TODO: fix this later\n").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();
    assert!(output.status.success());

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "some code\n// TODO: fix this later\n"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Fixed:"),
        "file was rewritten and should say so: {stdout}"
    );
    assert!(
        stdout.contains("TODO comment"),
        "TODO detection must not be dropped just because the file was also fixed: {stdout}"
    );
}

// issue #80: --stdin must apply fini.toml discovered from the cwd (and
// --config, if given), same as file mode. Previously stdin mode never
// loaded any config at all.
#[test]
fn test_issue_80() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("fini.toml"),
        "[normalize]\nfix_code_blocks = true\n",
    )
    .unwrap();

    let mut child = fini_cmd()
        .current_dir(dir.path())
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"```rust\nfn main() {}\n```\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout, "fn main() {}\n",
        "fix_code_blocks from the discovered fini.toml must apply to --stdin: {stdout}"
    );
}

// issue #80: --stdin must also honor an explicit --config path.
#[test]
fn test_issue_80_explicit_config() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = TempDir::new().unwrap();
    let config_path = dir.path().join("custom.toml");
    fs::write(&config_path, "[normalize]\nfix_code_blocks = true\n").unwrap();

    let mut child = fini_cmd()
        .arg("--stdin")
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"```rust\nfn main() {}\n```\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, "fn main() {}\n");
}

// issue #80 (follow-up): the issue #45 security-posture warning must still
// fire when the TOML that disables secret detection reaches --stdin through
// the same config-loading path this issue added.
#[test]
fn test_issue_80_stdin_warns_when_config_disables_secret_detection() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("fini.toml"),
        "[normalize]\ndetect_secrets = false\n",
    )
    .unwrap();

    let mut child = fini_cmd()
        .current_dir(dir.path())
        .arg("--stdin")
        .arg("--quiet")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("disables secret detection"),
        "missing security-posture warning on --stdin, even under --quiet: {stderr}"
    );
}

// issue #83: --check must print a reason line when the only change is line
// ending normalization (CRLF/CR -> LF), not just the bare "Error: <path>"
// header with nothing under it.
#[test]
fn test_issue_83() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\r\nline2\r\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("line endings normalized"),
        "must explain why the file failed --check: {stdout}"
    );
}

// issue #83 (follow-up): a lone `\r` (old Mac-style line ending, no `\n`)
// is itself a line ending that normalization collapses, and must not be
// misreported as "missing EOF newline" / "trailing whitespace" instead of
// (or alongside) the line-ending bullet.
#[test]
fn test_issue_83_lone_cr() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1\rline2\r").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("line endings normalized"),
        "must report the CR line endings: {stdout}"
    );
    assert!(
        !stdout.contains("missing EOF newline"),
        "the trailing \\r is a line ending, not a missing-newline case: {stdout}"
    );
    assert!(
        !stdout.contains("trailing whitespace"),
        "the trailing \\r must not be misread as trailing whitespace: {stdout}"
    );
}

// issue #83 (follow-up): an already-LF file with real trailing whitespace
// must not get a spurious line-ending bullet.
#[test]
fn test_issue_83_lf_only_no_crlf_bullet() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "line1  \nline2\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("line endings normalized"),
        "an LF-only file must not get a CRLF bullet: {stdout}"
    );
    assert!(stdout.contains("trailing whitespace at line 1"));
}

// issue #84: --init failing (e.g. fini.toml already exists) must exit 2
// (runtime error), matching the README's exit-code table, not exit 1
// (which the table reserves for --check finding problems).
#[test]
fn test_issue_84() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("fini.toml"), "existing").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg("--init")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
}

// issue #87: a filename containing a newline must not forge extra output
// lines — --quiet mode's contract is one path per line, and a crafted
// filename could otherwise spoof a fake result line for a different file.
#[cfg(unix)]
#[test]
fn test_issue_87() {
    let dir = TempDir::new().unwrap();
    let evil_name = "evil\nFixed: not_a_real_file.txt";
    let file = dir.path().join(evil_name);
    fs::write(&file, "hello").unwrap(); // missing EOF newline, so it's "fixed"

    let output = fini_cmd()
        .arg("--quiet")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "the embedded newline must not produce a second output line: {stdout:?}"
    );
    assert!(
        lines[0].contains("evil\\nFixed"),
        "the newline must be escaped, not printed literally: {stdout:?}"
    );
}

// issue #87 (follow-up): the diff header ("--- <path>" / "+++ <path>") must
// also be a single, escaped line, not just Quiet-mode output.
#[cfg(unix)]
#[test]
fn test_issue_87_diff_header() {
    let dir = TempDir::new().unwrap();
    let evil_name = "evil\nFixed: not_a_real_file.txt";
    let file = dir.path().join(evil_name);
    fs::write(&file, "hello").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let header_lines = stdout.lines().filter(|l| l.starts_with("--- ")).count();
    assert_eq!(
        header_lines, 1,
        "the embedded newline in the diff header must not add a header line: {stdout:?}"
    );
    assert!(stdout.contains("evil\\nFixed"), "{stdout:?}");
}

// issue #87 (follow-up): a per-entry walk error (e.g. permission denied)
// embeds the offending path in its Display text, which by the time it
// reaches "Error walking path: ..." is already a flattened String, not a
// Path — that message must still be escaped, not just safe_path_display's
// direct callers.
#[cfg(unix)]
#[test]
fn test_issue_87_walk_error_message() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let blocked = dir.path().join("evil\nFixed: not_a_real_file.txt");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("inner.txt"), "content").unwrap();
    fs::write(dir.path().join("good.txt"), "hello\n").unwrap();

    let mut perms = fs::metadata(&blocked).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&blocked, perms.clone()).unwrap();

    let output = fini_cmd()
        .arg("--quiet")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    perms.set_mode(0o755);
    fs::set_permissions(&blocked, perms).unwrap();

    // Running as root ignores the 000 permission, which would make the loop
    // below pass on empty stderr instead of actually exercising the fix.
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected a walk error (permission denied); stderr: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("evil\\nFixed"), "{stderr:?}");
    for line in stderr.lines() {
        assert!(
            line.starts_with("Error walking path:"),
            "the embedded newline must not forge an unrelated-looking stderr line: {stderr:?}"
        );
    }
}

// issue #89: --diff --quiet must not write files. --quiet alone selects
// Quiet display, which previously made should_write() see output_mode !=
// Diff and write anyway, silently overriding --diff's no-write guarantee.
#[test]
fn test_issue_89() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    let original = "hello"; // missing EOF newline
    fs::write(&file, original).unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg("--quiet")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        original,
        "--diff --quiet must not write the file"
    );
}

// issue #92: an empty target directory must not exit 0 silently - it should
// warn that nothing was scanned, so a typo'd path or exclude pattern that
// filters out everything doesn't look identical to a clean successful run.
#[test]
fn test_issue_92() {
    let dir = TempDir::new().unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no files matched"),
        "an empty target must not scan silently: {stderr:?}"
    );
}

// issue #92 (follow-up): the warning is a security-relevant "you scanned
// nothing" signal, like the issue #45 secret-detection-disabled warning -
// --quiet must not hide it either.
#[test]
fn test_issue_92_quiet_still_warns() {
    let dir = TempDir::new().unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--quiet")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no files matched"),
        "--quiet must not suppress the empty-scan warning: {stderr:?}"
    );
}

// issue #92: an --exclude pattern that matches every file in the target
// must warn the same way an empty directory does.
#[test]
fn test_issue_92_exclude_matches_everything() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--exclude")
        .arg("*")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no files matched"),
        "an exclude pattern matching everything must not scan silently: {stderr:?}"
    );
}

// issue #94: --check must not report "trailing whitespace" for a line whose
// only trailing character is Unicode whitespace the fixer never removes
// (remove_trailing_whitespace only trims ASCII space and tab).
#[test]
fn test_issue_94() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // U+00A0 (NBSP) trailing the line - not ASCII space/tab. No EOF newline
    // either, so the file has a *real* change (has_changes() true) and
    // print_change_summary_to actually runs - a file with zero changes at
    // all skips that function entirely, which would make this test pass
    // regardless of how the trailing-whitespace check is written.
    fs::write(&file, "hello\u{a0}").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing EOF newline"), "{stdout:?}");
    assert!(
        !stdout.contains("trailing whitespace"),
        "must not claim trailing whitespace for a line fix mode won't change: {stdout:?}"
    );

    // Confirm the fixer really does leave the NBSP untouched (only adding
    // the EOF newline), so the check above is verifying consistency and
    // not just an absence of any report.
    let fix_output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();
    assert!(fix_output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\u{a0}\n");
}

// issue #94 (follow-up): the reverse false-negative. A line ending in a
// fullwidth space (U+3000) has no ASCII space/tab at the raw-line level,
// but the fixer's earlier fullwidth-space-conversion step (issue #78's
// pipeline, step 6) turns it into one before trailing-whitespace removal
// runs (step 7) - so the fixer *does* trim it, and --check must say so.
#[test]
fn test_issue_94_fullwidth_space_trailing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello\u{3000}\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("trailing whitespace"),
        "a trailing fullwidth space becomes trimmable ASCII whitespace \
         once fullwidth-space conversion runs first, and must be reported: {stdout:?}"
    );

    let fix_output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();
    assert!(fix_output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

// issue #94 (follow-up): line_after_pre_trim_fixes must only strip
// zero-width characters when that fix is actually enabled - a zero-width
// space isn't ASCII whitespace, so with --keep-zero-width the fixer leaves
// it (and the space before it) untouched, and --check must not claim
// otherwise.
#[test]
fn test_issue_94_keep_zero_width() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    // No EOF newline, so the file has a real change regardless.
    fs::write(&file, "hello \u{200b}").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--keep-zero-width")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing EOF newline"), "{stdout:?}");
    assert!(
        !stdout.contains("trailing whitespace"),
        "with --keep-zero-width, the space before the ZWSP is not actually \
         trailing (the ZWSP survives after it) and must not be reported: {stdout:?}"
    );

    let fix_output = fini_cmd()
        .arg("--keep-zero-width")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(fix_output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello \u{200b}\n");
}

// Sibling of test_issue_94_keep_zero_width: the same input, without the
// flag, must still be reported and trimmed - default (remove_zero_width =
// true) behavior must not have regressed while fixing the flag-off case.
#[test]
fn test_issue_94_default_zero_width_removal_still_trims() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("test.txt");
    fs::write(&file, "hello \u{200b}").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("trailing whitespace"),
        "without --keep-zero-width, the ZWSP is removed first, exposing the \
         space as genuinely trailing: {stdout:?}"
    );

    let fix_output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();
    assert!(fix_output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

// issue #93: TOML `detect_secrets = false` disables *detection*, but must
// not also disable masking a secret's raw value out of --diff output -
// masking is an output guarantee, not part of the detection feature.
#[test]
fn test_issue_93() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("fini.toml"),
        "[normalize]\ndetect_secrets = false\n",
    )
    .unwrap();
    let file = dir.path().join("secret.txt");
    // A second, unrelated line with trailing whitespace produces a real
    // diff; the secret line alone wouldn't show one, since masking replaces
    // both sides of that line identically regardless of the whitespace fix.
    fs::write(&file, "password = \"hunter2hunter2\"\nother line   \n").unwrap();

    let output = fini_cmd()
        .current_dir(dir.path())
        .arg("--diff")
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("hunter2hunter2"),
        "the raw secret value must never reach --diff output, even with detect_secrets = false: {stdout:?}"
    );
    assert!(
        stdout.contains("[line masked: potential"),
        "masking should still run: {stdout:?}"
    );
}

// issue #93: same guarantee on the --stdin --check --diff path.
#[test]
fn test_issue_93_stdin() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("fini.toml"),
        "[normalize]\ndetect_secrets = false\n",
    )
    .unwrap();

    let mut child = fini_cmd()
        .current_dir(dir.path())
        .arg("--stdin")
        .arg("--check")
        .arg("--diff")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"password = \"hunter2hunter2\"\nother line   \n")
        .unwrap();

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("hunter2hunter2"),
        "the raw secret value must never reach --stdin --diff output either: {stderr:?}"
    );
    assert!(
        stderr.contains("[line masked: potential"),
        "masking should still visibly run on stdin, not just suppress the diff: {stderr:?}"
    );
}

// issue #95: `fini:ignore-next-line <kinds>` naming a specific kind must not
// also suppress an unrelated problem kind that happens to share the
// directive's own line, and the directive's own kind-list text ("todo" as a
// kind name) must not itself be misdetected as a real TODO marker.
#[test]
fn test_issue_95() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("sup.py");
    fs::write(
        &file,
        "TOKEN = \"sk_live_ABCDEFGHIJKLMNOPQRSTUVWXYZ\" # fini:ignore-next-line todo\nx = 1\n",
    )
    .unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    // `todo` was named, not `secret`, so the secret on the directive's own
    // line must still be reported and the check must fail.
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("suppressed by fini:ignore"),
        "a secret must not be suppressed by a directive naming only \"todo\": {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "the secret must still be reported: {stdout:?}"
    );
    assert!(
        !stdout.contains("problems suppressed"),
        "no phantom suppression should be counted: {stdout:?}"
    );
}

// issue #95: a clean file with only a `fini:ignore-next-line <kind>`
// directive must report zero suppressed problems - the directive's own
// kind-list text must not be misdetected as a marker and then "suppressed".
#[test]
fn test_issue_95_no_phantom_suppression_on_clean_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("k.py");
    fs::write(&file, "# fini:ignore-next-line todo\nx = 1\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("problems suppressed"),
        "a clean file must report zero suppressed problems: {stdout:?}"
    );
}

// issue #96: the GitHub Action must extract the fini binary into a
// dedicated temp directory (outside the consumer's checkout, which is the
// composite action's default working directory), and add only that
// directory to PATH - not the checkout root.
#[test]
fn test_issue_96() {
    let action_yaml =
        fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("action.yaml"))
            .unwrap();

    assert!(
        action_yaml.contains("INSTALL_DIR=\"$(mktemp -d"),
        "must extract into a dedicated temp directory, not the checkout root"
    );
    assert!(
        !action_yaml.contains("echo \"$PWD\" >> \"$GITHUB_PATH\""),
        "must not add the working directory (the consumer's checkout) to PATH"
    );
    assert!(
        action_yaml.contains("echo \"$INSTALL_DIR\" >> \"$GITHUB_PATH\""),
        "PATH must point at the dedicated install directory"
    );
}

// issue #97: verify-attestation must default to 'true' - the action's own
// default `version: latest` always resolves to an attested release, so a
// security control here should default to fail-closed rather than require
// an explicit opt-in. The `latest` version lookup must also authenticate
// its GitHub API call, since the unauthenticated rate limit (60/hour per
// IP, shared across a whole runner pool) made it intermittently fail.
#[test]
fn test_issue_97() {
    let action_yaml =
        fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("action.yaml"))
            .unwrap();

    let block_start = action_yaml
        .find("verify-attestation:")
        .expect("action.yaml must define a verify-attestation input");
    let key_indent = action_yaml[..block_start]
        .rfind('\n')
        .map_or(block_start, |nl| block_start - nl - 1);
    // Scope the scan to this input's own indented block: every following
    // line indented further than the key itself, stopping at the first
    // line indented the same or less (the next sibling input, or the end
    // of the `inputs:` map).
    let default_line = action_yaml[block_start..]
        .lines()
        .skip(1)
        .take_while(|l| {
            l.trim().is_empty() || l.chars().take_while(|c| *c == ' ').count() > key_indent
        })
        .find(|l| l.trim_start().starts_with("default:"))
        .expect("verify-attestation must set a default");
    let value = default_line
        .split_once(':')
        .unwrap()
        .1
        .trim()
        .trim_matches(|c| c == '\'' || c == '"');
    assert_eq!(
        value, "true",
        "verify-attestation must default to 'true': {default_line:?}"
    );

    assert!(
        action_yaml
            .lines()
            .any(|l| l.contains("Authorization: Bearer $GH_TOKEN")
                && l.contains("repos/tsukasaI/fini/releases/latest")),
        "the `latest` version lookup must authenticate its GitHub API call"
    );
}

// issue #98: closing stdout early (e.g. `fini --quiet dir | head -1`) must
// not panic. A pipe with no reader at all forces the very first write to
// fail, so a single file is enough - no dependence on output size exceeding
// the OS pipe buffer.
#[cfg(unix)]
#[test]
fn test_issue_98() {
    use std::io::Read;
    use std::os::unix::process::ExitStatusExt;
    use std::process::Stdio;

    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("f.txt"), "a   \n").unwrap();

    let (read_end, write_end) = std::io::pipe().unwrap();
    drop(read_end); // no reader: the first write gets EPIPE -> SIGPIPE

    let mut child = fini_cmd()
        .arg("--quiet")
        .arg(dir.path().to_str().unwrap())
        .stdout(Stdio::from(write_end))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    let status = child.wait().unwrap();

    assert!(
        !stderr.contains("panicked"),
        "must not panic on a closed stdout pipe: {stderr:?}"
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGPIPE),
        "must be killed by SIGPIPE, not exit with a Rust panic's default code: {stderr:?}, status: {status:?}"
    );
}

// issue #101: hidden files (.env, .github/workflows/*.yml) are silently
// excluded from a `fini --check .` secret-detection CI gate by default -
// --hidden opts a directory scan into covering them too.
#[test]
fn test_issue_101() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join(".env"),
        "AWS_SECRET_ACCESS_KEY = \"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\"\n",
    )
    .unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "hidden files are excluded by default"
    );

    let output = fini_cmd()
        .arg("--check")
        .arg("--hidden")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("potential secret"),
        "--hidden must scan .env: {stdout:?}"
    );
}

// issue #103: a file exceeding --max-file-size must be skipped like a
// binary file (counted, not fully buffered), instead of unconditionally
// reading and normalizing arbitrarily large files.
#[test]
fn test_issue_103() {
    let dir = TempDir::new().unwrap();
    let big = dir.path().join("big.txt");
    let small = dir.path().join("small.txt");
    fs::write(&big, "x".repeat(2000)).unwrap();
    fs::write(&small, "hello\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--max-file-size")
        .arg("1000")
        .arg("-v")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the oversized file must be skipped, not reported as a --check problem: {stdout:?}"
    );
    assert!(
        stdout.contains("too large"),
        "the oversized file must be reported as skipped: {stdout:?}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("Skipping") && l.contains("big.txt")),
        "big.txt must be named in a skip line: {stdout:?}"
    );
    assert_eq!(fs::read_to_string(&big).unwrap(), "x".repeat(2000));

    // The small file, under the limit, must not be reported as skipped.
    assert!(
        !stdout
            .lines()
            .any(|l| l.contains("Skipping") && l.contains("small.txt")),
        "small.txt is under the limit and must not be skipped: {stdout:?}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("Checked:") && l.contains("small.txt")),
        "small.txt must be reported as checked, not silently dropped: {stdout:?}"
    );
}

// issue #103: raising --max-file-size lets a file that would otherwise be
// skipped get processed normally.
#[test]
fn test_issue_103_raised_limit_allows_processing() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("medium.txt");
    fs::write(&file, "hello   \n").unwrap(); // trailing whitespace triggers a fix

    fini_cmd()
        .arg("--max-file-size")
        .arg("1")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "hello   \n",
        "a file over a tiny --max-file-size must be left untouched"
    );

    let output = fini_cmd()
        .arg("--max-file-size")
        .arg("1000")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), "hello\n");
}

// issue #104: --check --diff on a detection-only file (no content change,
// so no diff to show) must still print a filename header before the
// problem list - otherwise, with multiple files, there's no way to tell
// which line belongs to which file.
#[test]
fn test_issue_104() {
    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("a.py"), "# TODO: x\n").unwrap();
    fs::write(dir.path().join("b.py"), "password = \"hunter2hunter2\"\n").unwrap();

    let output = fini_cmd()
        .arg("--check")
        .arg("--diff")
        .arg(dir.path().to_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let a_pos = stdout.find("a.py");
    let todo_pos = stdout.find("TODO comment");
    let b_pos = stdout.find("b.py");
    let secret_pos = stdout.find("potential secret");
    assert!(
        a_pos.is_some() && todo_pos.is_some() && a_pos < todo_pos,
        "the a.py header must precede its TODO line: {stdout:?}"
    );
    assert!(
        b_pos.is_some() && secret_pos.is_some() && b_pos < secret_pos,
        "the b.py header must precede its secret line: {stdout:?}"
    );
    assert!(
        !stdout.contains("hunter2hunter2"),
        "the raw secret value must never reach --diff output: {stdout:?}"
    );
}

// issue #171: fix-mode --diff must list detection problems like check mode
// does, and must not print a hunkless `---`/`+++` pair when there is no diff.
#[test]
fn test_fix_diff_lists_detections_without_empty_diff_header() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.py");
    fs::write(&file, "// TODO: x\n").unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TODO comment at line 1"),
        "detection missing from --diff output: {stdout:?}"
    );
    assert!(
        !stdout.contains("--- ") && !stdout.contains("+++ "),
        "hunkless diff header must be omitted: {stdout:?}"
    );
    assert!(
        stdout.contains("Detected: ") && stdout.contains("a.py"),
        "Detected header missing from --diff output: {stdout:?}"
    );
    assert!(
        !stdout.contains("Would fix:"),
        "an unchanged file must not be labelled Would fix: {stdout:?}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "// TODO: x\n");
}

// issue #171: a real diff and the detection list must both be shown.
#[test]
fn test_fix_diff_shows_hunk_and_detections_together() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("a.py");
    fs::write(&file, "x = 1  \n// TODO: x\n").unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--- ") && stdout.contains("+++ ") && stdout.contains("-x = 1  "),
        "diff hunk missing from --diff output: {stdout:?}"
    );
    assert!(
        stdout.contains("TODO comment at line 2"),
        "detection missing from --diff output: {stdout:?}"
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), "x = 1  \n// TODO: x\n");
}

// issue #171: when masking collapses the diff, fix-mode --diff names the file
// and still lists the problems, without leaking the secret value.
#[test]
fn test_fix_diff_masked_collapse_lists_problems_under_header() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("b.txt");
    fs::write(&file, "password = \"hunter2hunter2\"  \n").unwrap();

    let output = fini_cmd()
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let header_pos = stdout.find("Would fix:");
    let secret_pos = stdout.find("potential secret");
    assert!(
        header_pos.is_some() && secret_pos.is_some() && header_pos < secret_pos,
        "header must precede the secret hint: {stdout:?}"
    );
    assert!(
        stdout.contains("b.txt"),
        "file name missing from --diff output: {stdout:?}"
    );
    assert!(
        !stdout.contains("--- ") && !stdout.contains("hunter2hunter2"),
        "masked collapse must print no hunk and no raw value: {stdout:?}"
    );
}

// issue #193: a masked-away diff still says what would be fixed, via the
// content-free change summary, in fix mode and in check mode.
#[test]
fn test_fix_diff_masked_collapse_lists_change_summary() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("fini.toml");
    let file = dir.path().join("b.txt");
    fs::write(&config, "[normalize]\ndetect_secrets = false\n").unwrap();
    fs::write(&file, "password = \"hunter2hunter2\"  \n").unwrap();

    let output = fini_cmd()
        .arg("--config")
        .arg(config.to_str().unwrap())
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let header_pos = stdout.find("Would fix:");
    let bullet_pos = stdout.find("  - trailing whitespace at line 1");
    assert!(
        header_pos.is_some() && bullet_pos.is_some() && header_pos < bullet_pos,
        "change summary must follow the header: {stdout:?}"
    );
    assert_eq!(
        stdout.matches("  - trailing whitespace at line 1").count(),
        1,
        "bullet must appear exactly once: {stdout:?}"
    );
    assert!(
        !stdout.contains("--- ")
            && !stdout.contains("hunter2hunter2")
            && !stderr.contains("hunter2hunter2"),
        "masked collapse must print no hunk and no raw value: {stdout:?} {stderr:?}"
    );
}

#[test]
fn test_check_diff_masked_collapse_lists_change_summary() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("fini.toml");
    let file = dir.path().join("b.txt");
    fs::write(&config, "[normalize]\ndetect_secrets = false\n").unwrap();
    fs::write(&file, "password = \"hunter2hunter2\"  \n").unwrap();

    let output = fini_cmd()
        .arg("--config")
        .arg(config.to_str().unwrap())
        .arg("--check")
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let header_pos = stdout.find("Error:");
    let bullet_pos = stdout.find("  - trailing whitespace at line 1");
    assert!(
        header_pos.is_some() && bullet_pos.is_some() && header_pos < bullet_pos,
        "change summary must follow the header: {stdout:?}"
    );
    assert_eq!(
        stdout.matches("  - trailing whitespace at line 1").count(),
        1,
        "bullet must appear exactly once: {stdout:?}"
    );
    assert!(
        !stdout.contains("--- ")
            && !stdout.contains("hunter2hunter2")
            && !stderr.contains("hunter2hunter2"),
        "masked collapse must print no hunk and no raw value: {stdout:?} {stderr:?}"
    );
}

// issue #215: a partly masked diff must not hide the change on the masked
// line - the line-1 hunk shows, and the summary names line 2.
#[test]
fn test_diff_partly_masked_lists_change_summary_for_masked_line() {
    for check in [false, true] {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("fini.toml");
        let file = dir.path().join("b.txt");
        fs::write(&config, "[normalize]\ndetect_secrets = false\n").unwrap();
        fs::write(&file, "x = 1  \npassword = \"hunter2hunter2\"  \n").unwrap();

        let mut cmd = fini_cmd();
        cmd.arg("--config").arg(config.to_str().unwrap());
        if check {
            cmd.arg("--check");
        }
        let output = cmd
            .arg("--diff")
            .arg(file.to_str().unwrap())
            .output()
            .unwrap();

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("--- ") && stdout.contains("-x = 1  "),
            "line-1 hunk missing (check={check}): {stdout:?}"
        );
        assert_eq!(
            stdout.matches("  - trailing whitespace at line 2").count(),
            1,
            "line-2 summary bullet missing (check={check}): {stdout:?}"
        );
        assert!(
            !stdout.contains("hunter2hunter2") && !stderr.contains("hunter2hunter2"),
            "raw secret leaked (check={check}): {stdout:?} {stderr:?}"
        );
    }
}

// issue #185: with secret detection off there are no problems, but the file
// is still counted as fixed, so --diff must still name it.
#[test]
fn test_fix_diff_masked_collapse_without_problems_still_names_file() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("fini.toml");
    let file = dir.path().join("b.txt");
    fs::write(&config, "[normalize]\ndetect_secrets = false\n").unwrap();
    fs::write(&file, "password = \"hunter2hunter2\"  \n").unwrap();

    let output = fini_cmd()
        .arg("--config")
        .arg(config.to_str().unwrap())
        .arg("--diff")
        .arg(file.to_str().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Would fix:") && stdout.contains("b.txt"),
        "file name missing from --diff output: {stdout:?}"
    );
    assert!(
        !stdout.contains("hunter2hunter2"),
        "the raw secret value must never reach --diff output: {stdout:?}"
    );
}

// ===========================================
// Inherited git env vs. tracked-file rescue (issue #225)
// ===========================================

// Literal list on purpose (not walker's GIT_ENV_TO_STRIP): deriving it from
// the production list would stay green if a var were dropped there.
const INHERITED_GIT_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

fn clean_git(dir: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    for var in INHERITED_GIT_ENV {
        cmd.env_remove(var);
    }
    let out = cmd.arg("-C").arg(dir).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// issue #225: the strip in the production `git ls-files` call must hold when
// fini itself inherits redirecting GIT_* vars (e.g. when run from a git
// hook). Exercises the real path end-to-end, unlike the walker unit test.
#[test]
fn test_issue_225_tracked_gitignored_rescue_survives_inherited_git_env() {
    let dir = TempDir::new().unwrap();
    clean_git(dir.path(), &["init", "-q"]);
    fs::write(dir.path().join("ignored.txt"), "// TODO: rescued\n").unwrap();
    clean_git(dir.path(), &["add", "ignored.txt"]);
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();

    let bogus = dir.path().join("nonexistent-git-env");
    let mut cmd = fini_cmd();
    for var in INHERITED_GIT_ENV {
        cmd.env(var, &bogus);
    }
    let output = cmd.arg("--check").arg(dir.path()).output().unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "a TODO finding under --check must exit 1: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("git ls-files") && !stderr.contains("could not run git"),
        "git failure warning leaked: {stderr:?}"
    );
    assert!(
        stdout.contains("ignored.txt") && stdout.contains("TODO"),
        "tracked-but-gitignored TODO not reported: {stdout:?} {stderr:?}"
    );
}
