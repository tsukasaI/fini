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
    /// If Some(true), skip TODO detection
    pub no_detect_todos: Option<bool>,
    /// If Some(true), skip FIXME detection
    pub no_detect_fixmes: Option<bool>,
    /// If Some(true), skip debug code detection
    pub no_detect_debug: Option<bool>,
    /// If Some(true), include console.error/eprintln in debug detection
    pub strict_debug: Option<bool>,
    /// If Some(true), skip secret pattern detection
    pub no_detect_secrets: Option<bool>,
    /// Maximum line length
    pub max_line_length: Option<usize>,
}

/// Merge configurations from CLI, TOML, and defaults.
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
        detect_todos: cli
            .no_detect_todos
            .map(|no| !no)
            .or_else(|| toml.and_then(|t| t.detect_todos))
            .unwrap_or(defaults.detect_todos),
        detect_fixmes: cli
            .no_detect_fixmes
            .map(|no| !no)
            .or_else(|| toml.and_then(|t| t.detect_fixmes))
            .unwrap_or(defaults.detect_fixmes),
        detect_debug: cli
            .no_detect_debug
            .map(|no| !no)
            .or_else(|| toml.and_then(|t| t.detect_debug))
            .unwrap_or(defaults.detect_debug),
        strict_debug: cli
            .strict_debug
            .or_else(|| toml.and_then(|t| t.strict_debug))
            .unwrap_or(defaults.strict_debug),
        detect_secrets: cli
            .no_detect_secrets
            .map(|no| !no)
            .or_else(|| toml.and_then(|t| t.detect_secrets))
            .unwrap_or(defaults.detect_secrets),
        max_line_length: cli
            .max_line_length
            .or_else(|| toml.and_then(|t| t.max_line_length))
            .or(defaults.max_line_length),
    }
}

/// Merge exclude patterns from CLI and TOML.
///
/// Priority: CLI > TOML > default (empty). Unlike `merge_normalize_config`,
/// this isn't a field-by-field merge - a non-empty CLI `--exclude` list wins
/// outright, since there's no per-pattern precedence to reconcile.
pub fn merge_exclude_patterns(
    cli_exclude: &[String],
    toml_exclude: Option<&[String]>,
) -> Vec<String> {
    if !cli_exclude.is_empty() {
        cli_exclude.to_vec()
    } else {
        toml_exclude.map(<[String]>::to_vec).unwrap_or_default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let toml = NormalizeSection {
            max_blank_lines: Some(2),
            remove_zero_width: Some(true),
            remove_leading_blanks: Some(false),
            fix_code_blocks: Some(true),
            ..Default::default()
        };

        let config = merge_normalize_config(&cli, Some(&toml));

        assert_eq!(config.max_blank_lines, Some(5)); // CLI wins
        assert!(!config.remove_zero_width); // CLI (keep=true -> remove=false)
        assert!(!config.remove_leading_blanks); // TOML (CLI not set)
        assert!(!config.fix_code_blocks); // CLI wins
    }

    #[test]
    fn test_merge_phase3_defaults() {
        let cli = CliNormalizeOptions::default();
        let config = merge_normalize_config(&cli, None);

        assert!(config.detect_todos);
        assert!(config.detect_fixmes);
        assert!(config.detect_debug);
        assert!(!config.strict_debug);
        assert!(config.detect_secrets);
        assert_eq!(config.max_line_length, None);
    }

    #[test]
    fn test_merge_phase3_toml_overrides() {
        let cli = CliNormalizeOptions::default();
        let toml = NormalizeSection {
            detect_todos: Some(false),
            detect_debug: Some(false),
            strict_debug: Some(true),
            detect_secrets: Some(false),
            max_line_length: Some(120),
            ..Default::default()
        };

        let config = merge_normalize_config(&cli, Some(&toml));

        assert!(!config.detect_todos);
        assert!(config.detect_fixmes); // default
        assert!(!config.detect_debug);
        assert!(config.strict_debug);
        assert!(!config.detect_secrets);
        assert_eq!(config.max_line_length, Some(120));
    }

    #[test]
    fn test_merge_phase3_cli_overrides_toml() {
        let cli = CliNormalizeOptions {
            no_detect_todos: Some(true),
            no_detect_debug: Some(false), // explicitly enable
            strict_debug: Some(true),
            max_line_length: Some(80),
            ..Default::default()
        };
        let toml = NormalizeSection {
            detect_todos: Some(true),
            detect_debug: Some(false),
            strict_debug: Some(false),
            max_line_length: Some(120),
            ..Default::default()
        };

        let config = merge_normalize_config(&cli, Some(&toml));

        assert!(!config.detect_todos); // CLI no_detect=true -> detect=false
        assert!(config.detect_debug); // CLI no_detect=false -> detect=true
        assert!(config.strict_debug); // CLI wins
        assert_eq!(config.max_line_length, Some(80)); // CLI wins
    }

    #[test]
    fn test_merge_cli_only() {
        let cli = CliNormalizeOptions {
            max_blank_lines: Some(1),
            keep_zero_width: Some(false),
            keep_leading_blanks: Some(false),
            fix_code_blocks: Some(true),
            ..Default::default()
        };

        let config = merge_normalize_config(&cli, None);

        assert_eq!(config.max_blank_lines, Some(1));
        assert!(config.remove_zero_width); // keep=false -> remove=true
        assert!(config.remove_leading_blanks); // keep=false -> remove=true
        assert!(config.fix_code_blocks);
    }

    #[test]
    fn test_merge_exclude_defaults_only() {
        let result = merge_exclude_patterns(&[], None);
        assert_eq!(result, Vec::<String>::new());
    }

    #[test]
    fn test_merge_exclude_toml_overrides_defaults() {
        let toml = vec!["*.log".to_string(), "dist/".to_string()];
        let result = merge_exclude_patterns(&[], Some(&toml));
        assert_eq!(result, toml);
    }

    #[test]
    fn test_merge_exclude_cli_overrides_toml() {
        let cli = vec!["*.tmp".to_string()];
        let toml = vec!["*.log".to_string(), "dist/".to_string()];
        let result = merge_exclude_patterns(&cli, Some(&toml));
        assert_eq!(result, cli);
    }

    #[test]
    fn test_merge_exclude_cli_only() {
        let cli = vec!["*.tmp".to_string()];
        let result = merge_exclude_patterns(&cli, None);
        assert_eq!(result, cli);
    }
}
