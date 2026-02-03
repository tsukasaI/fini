//! Template generation for `--init` command

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Template fini.toml with documentation
pub const FINI_TOML_TEMPLATE: &str = r#"# fini.toml - Configuration for fini file normalizer
# https://github.com/tsukasaI/fini
#
# fini normalizes text files by:
# - Ensuring files end with a single newline
# - Converting CRLF/CR line endings to LF
# - Removing trailing whitespace from lines
# - Converting full-width spaces to regular spaces
#
# These behaviors are always enabled. The settings below control
# optional features - uncomment and modify as needed.

[normalize]
# Maximum consecutive blank lines allowed.
# Set to 0 to remove all blank lines, or comment out for no limit.
# max_blank_lines = 2

# Remove zero-width characters (ZWSP, ZWJ, ZWNJ, etc.)
# Useful for cleaning up text copied from web pages or word processors.
# Default: true
# remove_zero_width = true

# Remove leading blank lines at the start of files.
# Default: true
# remove_leading_blanks = true

# Remove markdown code block markers (``` fences).
# Enable when extracting code from AI assistant responses.
# Default: false
# fix_code_blocks = false
"#;

/// Generate fini.toml in the specified directory (or current directory if None).
///
/// Returns an error if fini.toml already exists.
pub fn generate_init_file_in(dir: Option<&Path>) -> io::Result<PathBuf> {
    let path = dir.map_or_else(|| PathBuf::from("fini.toml"), |d| d.join("fini.toml"));

    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "fini.toml already exists",
        ));
    }

    fs::write(&path, FINI_TOML_TEMPLATE)?;
    Ok(path)
}

/// Generate fini.toml in the current directory.
///
/// Returns an error if fini.toml already exists.
pub fn generate_init_file() -> io::Result<PathBuf> {
    generate_init_file_in(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_generate_init_file_creates_file() {
        let dir = TempDir::new().unwrap();

        let result = generate_init_file_in(Some(dir.path()));
        assert!(result.is_ok());

        let path = result.unwrap();
        assert!(path.exists());
        assert_eq!(path, dir.path().join("fini.toml"));

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("[normalize]"));
        assert!(content.contains("max_blank_lines"));
    }

    #[test]
    fn test_generate_init_file_fails_if_exists() {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("fini.toml");

        // Create existing file
        fs::write(&config_path, "existing").unwrap();

        let result = generate_init_file_in(Some(dir.path()));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn test_template_is_valid_toml() {
        // Verify the template can be parsed
        let parsed: Result<super::super::toml_schema::FiniToml, _> =
            toml::from_str(FINI_TOML_TEMPLATE);
        assert!(parsed.is_ok());
    }
}
