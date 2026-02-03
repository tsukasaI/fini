//! Configuration file support for fini.
//!
//! This module provides:
//! - Loading configuration from `fini.toml`
//! - Config file discovery (search upward from current directory)
//! - Merging CLI args, config file, and defaults
//! - Template generation with `--init`
//! - `.editorconfig` reading for migration assistance

mod editorconfig;
mod file;
mod init;
mod merge;
mod toml_schema;

pub use editorconfig::{check_editorconfig_conflicts, find_editorconfig, parse_editorconfig};
pub use file::{find_config_file, find_file_upward, load_config, ConfigError};
pub use init::{generate_init_file, FINI_TOML_TEMPLATE};
pub use merge::{merge_normalize_config, CliNormalizeOptions};
pub use toml_schema::{FiniToml, NormalizeSection};
