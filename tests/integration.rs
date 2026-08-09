use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn fini_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fini"))
}

// ===========================================
// Phase 3: CLI Integration Tests
// ===========================================

#[test]
fn test_version_output_shape_is_stable() {
    // The VS Code extension parses `fini --version` on activation for its
    // compatibility check (issue #43) — keep the `fini X.Y.Z` shape stable.
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
fn test_skip_empty_files() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("empty.txt");
    fs::write(&file, "").unwrap();

    let output = fini_cmd().arg(file.to_str().unwrap()).output().unwrap();

    // Empty file should stay empty
    assert_eq!(fs::read_to_string(&file).unwrap(), "");

    // Should succeed
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

    assert!(!output.status.success());
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
    assert!(stderr.contains("Error writing"), "stderr: {stderr}");
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
