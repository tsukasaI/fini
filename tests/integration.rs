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
