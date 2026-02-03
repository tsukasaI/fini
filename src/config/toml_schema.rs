//! TOML schema definitions for fini.toml

use serde::{Deserialize, Serialize};

/// Root structure for fini.toml
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FiniToml {
    /// Normalization settings
    #[serde(default)]
    pub normalize: NormalizeSection,
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
}
