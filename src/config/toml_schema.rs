//! TOML schema definitions for fini.toml

use serde::{Deserialize, Serialize};

/// Root structure for fini.toml
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FiniToml {
    /// Normalization settings
    #[serde(default)]
    pub normalize: NormalizeSection,

    /// File exclusion patterns (gitignore-style globs)
    #[serde(default)]
    pub exclude: Option<Vec<String>>,
}

/// `[normalize]` section in fini.toml
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct NormalizeSection {
    /// Maximum consecutive blank lines (None = no limit)
    pub max_blank_lines: Option<usize>,

    /// Remove zero-width characters (default: true)
    pub remove_zero_width: Option<bool>,

    /// Remove leading blank lines (default: true)
    pub remove_leading_blanks: Option<bool>,

    /// Remove code block remnants (default: false)
    pub fix_code_blocks: Option<bool>,

    /// Detect TODO comments (default: true)
    pub detect_todos: Option<bool>,

    /// Detect FIXME comments (default: true)
    pub detect_fixmes: Option<bool>,

    /// Detect debug code (default: true)
    pub detect_debug: Option<bool>,

    /// Include console.error/eprintln in debug detection (default: false)
    pub strict_debug: Option<bool>,

    /// Detect secret patterns (default: true)
    pub detect_secrets: Option<bool>,

    /// Maximum line length (None = disabled)
    pub max_line_length: Option<usize>,
}
