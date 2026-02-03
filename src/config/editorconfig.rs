//! .editorconfig parsing for migration assistance

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::file::find_file_upward;

/// Relevant settings extracted from .editorconfig
#[derive(Debug, Default)]
pub struct EditorConfigSettings {
    pub trim_trailing_whitespace: Option<bool>,
    pub insert_final_newline: Option<bool>,
    pub end_of_line: Option<String>,
}

/// Find .editorconfig by searching upward from the given directory.
pub fn find_editorconfig(start_dir: &Path) -> Option<PathBuf> {
    find_file_upward(start_dir, ".editorconfig", false)
}

/// Parse .editorconfig file and extract relevant settings.
///
/// Only parses the `[*]` section (global settings) for simplicity.
pub fn parse_editorconfig(path: &Path) -> io::Result<EditorConfigSettings> {
    let content = fs::read_to_string(path)?;
    let mut settings = EditorConfigSettings::default();
    let mut in_global_section = false;

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        // Section header
        if line.starts_with('[') && line.ends_with(']') {
            // [*] applies to all files
            in_global_section = line == "[*]";
            continue;
        }

        // Only process [*] section
        if !in_global_section {
            continue;
        }

        // Parse key = value
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim().to_lowercase();

            match key.as_str() {
                "trim_trailing_whitespace" => {
                    settings.trim_trailing_whitespace = Some(value == "true");
                }
                "insert_final_newline" => {
                    settings.insert_final_newline = Some(value == "true");
                }
                "end_of_line" => {
                    settings.end_of_line = Some(value);
                }
                _ => {}
            }
        }
    }

    Ok(settings)
}

/// Check for conflicts between .editorconfig and fini's fixed behaviors.
///
/// Returns a list of warning messages for conflicting settings.
pub fn check_editorconfig_conflicts(settings: &EditorConfigSettings) -> Vec<String> {
    let mut warnings = Vec::new();

    if settings.trim_trailing_whitespace == Some(false) {
        warnings
            .push("editorconfig has trim_trailing_whitespace=false, but fini always trims".into());
    }

    if settings.insert_final_newline == Some(false) {
        warnings
            .push("editorconfig has insert_final_newline=false, but fini always inserts".into());
    }

    if let Some(eol) = &settings.end_of_line {
        if eol != "lf" {
            warnings.push(format!(
                "editorconfig has end_of_line={eol}, but fini normalizes to LF"
            ));
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_find_editorconfig() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join(".editorconfig");
        fs::write(&config_path, "root = true\n").unwrap();

        let found = find_editorconfig(dir.path());
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn test_find_editorconfig_in_parent() {
        let parent = TempDir::new().unwrap();
        let config_path = parent.path().join(".editorconfig");
        fs::write(&config_path, "root = true\n").unwrap();

        let child = parent.path().join("subdir");
        fs::create_dir(&child).unwrap();

        let found = find_editorconfig(&child);
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn test_parse_editorconfig_global_section() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join(".editorconfig");
        fs::write(
            &config_path,
            r#"
root = true

[*]
trim_trailing_whitespace = true
insert_final_newline = true
end_of_line = lf

[*.md]
trim_trailing_whitespace = false
"#,
        )
        .unwrap();

        let settings = parse_editorconfig(&config_path).unwrap();
        assert_eq!(settings.trim_trailing_whitespace, Some(true));
        assert_eq!(settings.insert_final_newline, Some(true));
        assert_eq!(settings.end_of_line, Some("lf".to_string()));
    }

    #[test]
    fn test_parse_editorconfig_no_global_section() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join(".editorconfig");
        fs::write(
            &config_path,
            r#"
root = true

[*.js]
indent_style = space
"#,
        )
        .unwrap();

        let settings = parse_editorconfig(&config_path).unwrap();
        assert_eq!(settings.trim_trailing_whitespace, None);
        assert_eq!(settings.insert_final_newline, None);
    }

    #[test]
    fn test_check_conflicts_none() {
        let settings = EditorConfigSettings {
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(true),
            end_of_line: Some("lf".to_string()),
        };

        let warnings = check_editorconfig_conflicts(&settings);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_check_conflicts_all() {
        let settings = EditorConfigSettings {
            trim_trailing_whitespace: Some(false),
            insert_final_newline: Some(false),
            end_of_line: Some("crlf".to_string()),
        };

        let warnings = check_editorconfig_conflicts(&settings);
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].contains("trim_trailing_whitespace"));
        assert!(warnings[1].contains("insert_final_newline"));
        assert!(warnings[2].contains("end_of_line"));
    }

    #[test]
    fn test_check_conflicts_partial() {
        let settings = EditorConfigSettings {
            trim_trailing_whitespace: None,
            insert_final_newline: Some(false),
            end_of_line: None,
        };

        let warnings = check_editorconfig_conflicts(&settings);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("insert_final_newline"));
    }
}
