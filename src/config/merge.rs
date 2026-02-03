//! Configuration merging logic
//!
//! Priority: CLI args > fini.toml > defaults

use crate::NormalizeConfig;

use super::toml_schema::NormalizeSection;

/// CLI options that can override config file settings.
///
/// Uses `Option<T>` to distinguish "not specified" from "explicitly set".
#[derive(Debug, Default)]
pub struct CliNormalizeOptions {
    pub max_blank_lines: Option<usize>,
    /// If Some(true), keep zero-width chars (inverted in config)
    pub keep_zero_width: Option<bool>,
    /// If Some(true), keep leading blanks (inverted in config)
    pub keep_leading_blanks: Option<bool>,
    pub fix_code_blocks: Option<bool>,
}

/// Merge configurations from CLI, TOML, and defaults.
///
/// Priority: CLI > TOML > defaults
pub fn merge_normalize_config(
    cli: &CliNormalizeOptions,
    toml: Option<&NormalizeSection>,
) -> NormalizeConfig {
    let defaults = NormalizeConfig::default();

    NormalizeConfig {
        max_blank_lines: cli
            .max_blank_lines
            .or_else(|| toml.and_then(|t| t.max_blank_lines))
            .or(defaults.max_blank_lines),
        remove_zero_width: cli
            .keep_zero_width
            .map(|keep| !keep)
            .or_else(|| toml.and_then(|t| t.remove_zero_width))
            .unwrap_or(defaults.remove_zero_width),
        remove_leading_blanks: cli
            .keep_leading_blanks
            .map(|keep| !keep)
            .or_else(|| toml.and_then(|t| t.remove_leading_blanks))
            .unwrap_or(defaults.remove_leading_blanks),
        fix_code_blocks: cli
            .fix_code_blocks
            .or_else(|| toml.and_then(|t| t.fix_code_blocks))
            .unwrap_or(defaults.fix_code_blocks),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_defaults_only() {
        let cli = CliNormalizeOptions::default();
        let config = merge_normalize_config(&cli, None);

        assert_eq!(config.max_blank_lines, None);
        assert!(config.remove_zero_width);
        assert!(config.remove_leading_blanks);
        assert!(!config.fix_code_blocks);
    }

    #[test]
    fn test_merge_toml_overrides_defaults() {
        let cli = CliNormalizeOptions::default();
        let toml = NormalizeSection {
            max_blank_lines: Some(2),
            remove_zero_width: Some(false),
            remove_leading_blanks: None,
            fix_code_blocks: Some(true),
        };

        let config = merge_normalize_config(&cli, Some(&toml));

        assert_eq!(config.max_blank_lines, Some(2));
        assert!(!config.remove_zero_width);
        assert!(config.remove_leading_blanks); // default
        assert!(config.fix_code_blocks);
    }

    #[test]
    fn test_merge_cli_overrides_toml() {
        let cli = CliNormalizeOptions {
            max_blank_lines: Some(5),
            keep_zero_width: Some(true), // keep = true -> remove = false
            keep_leading_blanks: None,
            fix_code_blocks: Some(false),
        };
        let toml = NormalizeSection {
            max_blank_lines: Some(2),
            remove_zero_width: Some(true),
            remove_leading_blanks: Some(false),
            fix_code_blocks: Some(true),
        };

        let config = merge_normalize_config(&cli, Some(&toml));

        assert_eq!(config.max_blank_lines, Some(5)); // CLI wins
        assert!(!config.remove_zero_width); // CLI (keep=true -> remove=false)
        assert!(!config.remove_leading_blanks); // TOML (CLI not set)
        assert!(!config.fix_code_blocks); // CLI wins
    }

    #[test]
    fn test_merge_cli_only() {
        let cli = CliNormalizeOptions {
            max_blank_lines: Some(1),
            keep_zero_width: Some(false),
            keep_leading_blanks: Some(false),
            fix_code_blocks: Some(true),
        };

        let config = merge_normalize_config(&cli, None);

        assert_eq!(config.max_blank_lines, Some(1));
        assert!(config.remove_zero_width); // keep=false -> remove=true
        assert!(config.remove_leading_blanks); // keep=false -> remove=true
        assert!(config.fix_code_blocks);
    }
}
