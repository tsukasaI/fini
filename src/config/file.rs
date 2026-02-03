//! Config file discovery and loading

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::toml_schema::FiniToml;

/// Error type for configuration loading
#[derive(Debug)]
pub enum ConfigError {
    /// IO error reading the file
    Io(io::Error),
    /// TOML parsing error
    Parse(toml::de::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "failed to read config file: {e}"),
            ConfigError::Parse(e) => write!(f, "failed to parse config file: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io(e) => Some(e),
            ConfigError::Parse(e) => Some(e),
        }
    }
}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        ConfigError::Parse(e)
    }
}

/// Search upward from `start_dir` for a file with the given name.
///
/// If `stop_at_git_root` is true, stops searching when a `.git` directory is found.
/// Returns `None` if the file is not found.
pub fn find_file_upward(
    start_dir: &Path,
    filename: &str,
    stop_at_git_root: bool,
) -> Option<PathBuf> {
    let mut current = start_dir.to_path_buf();

    loop {
        let file_path = current.join(filename);
        if file_path.exists() {
            return Some(file_path);
        }

        if stop_at_git_root && current.join(".git").exists() {
            return None;
        }

        if !current.pop() {
            return None;
        }
    }
}

/// Find fini.toml by searching upward from the given directory.
///
/// Stops at the first `fini.toml` found, or at the git repository root
/// (directory containing `.git`), whichever comes first.
///
/// Returns `None` if no config file is found.
pub fn find_config_file(start_dir: &Path) -> Option<PathBuf> {
    find_file_upward(start_dir, "fini.toml", true)
}

/// Load and parse fini.toml from the given path.
pub fn load_config(path: &Path) -> Result<FiniToml, ConfigError> {
    let content = fs::read_to_string(path)?;
    let config: FiniToml = toml::from_str(&content)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_find_config_in_current_dir() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fini.toml");
        fs::write(&config_path, "[normalize]\n").unwrap();

        let found = find_config_file(dir.path());
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn test_find_config_in_parent_dir() {
        let parent = TempDir::new().unwrap();
        let config_path = parent.path().join("fini.toml");
        fs::write(&config_path, "[normalize]\n").unwrap();

        let child = parent.path().join("subdir");
        fs::create_dir(&child).unwrap();

        let found = find_config_file(&child);
        assert_eq!(found, Some(config_path));
    }

    #[test]
    fn test_find_config_stops_at_git_root() {
        let dir = TempDir::new().unwrap();
        // Create .git directory to mark git root
        fs::create_dir(dir.path().join(".git")).unwrap();

        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();

        // No config in this tree
        let found = find_config_file(&subdir);
        assert_eq!(found, None);
    }

    #[test]
    fn test_find_config_prefers_closer() {
        let parent = TempDir::new().unwrap();
        let parent_config = parent.path().join("fini.toml");
        fs::write(&parent_config, "[normalize]\nfix_code_blocks = false\n").unwrap();

        let child = parent.path().join("subdir");
        fs::create_dir(&child).unwrap();
        let child_config = child.join("fini.toml");
        fs::write(&child_config, "[normalize]\nfix_code_blocks = true\n").unwrap();

        let found = find_config_file(&child);
        assert_eq!(found, Some(child_config));
    }

    #[test]
    fn test_load_config_full() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fini.toml");
        fs::write(
            &config_path,
            r#"
[normalize]
max_blank_lines = 2
remove_zero_width = false
remove_leading_blanks = true
fix_code_blocks = true
"#,
        )
        .unwrap();

        let config = load_config(&config_path).unwrap();
        assert_eq!(config.normalize.max_blank_lines, Some(2));
        assert_eq!(config.normalize.remove_zero_width, Some(false));
        assert_eq!(config.normalize.remove_leading_blanks, Some(true));
        assert_eq!(config.normalize.fix_code_blocks, Some(true));
    }

    #[test]
    fn test_load_config_partial() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fini.toml");
        fs::write(
            &config_path,
            r#"
[normalize]
fix_code_blocks = true
"#,
        )
        .unwrap();

        let config = load_config(&config_path).unwrap();
        assert_eq!(config.normalize.max_blank_lines, None);
        assert_eq!(config.normalize.remove_zero_width, None);
        assert_eq!(config.normalize.remove_leading_blanks, None);
        assert_eq!(config.normalize.fix_code_blocks, Some(true));
    }

    #[test]
    fn test_load_config_empty() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fini.toml");
        fs::write(&config_path, "").unwrap();

        let config = load_config(&config_path).unwrap();
        assert_eq!(config.normalize.max_blank_lines, None);
        assert_eq!(config.normalize.fix_code_blocks, None);
    }

    #[test]
    fn test_load_config_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fini.toml");
        fs::write(&config_path, "invalid toml {{{\n").unwrap();

        let result = load_config(&config_path);
        assert!(matches!(result, Err(ConfigError::Parse(_))));
    }
}
