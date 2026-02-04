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
