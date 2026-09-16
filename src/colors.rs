use std::io::{self, IsTerminal};

const RESET: &str = "\x1b[0m";

#[derive(Clone, Copy)]
pub struct Colors {
    pub error: &'static str,
    pub warning: &'static str,
    pub success: &'static str,
    pub info: &'static str,
    enabled: bool,
}

impl Colors {
    pub fn new(enabled: bool) -> Self {
        if enabled {
            Self {
                error: "\x1b[31m",   // Red
                warning: "\x1b[33m", // Yellow
                success: "\x1b[32m", // Green
                info: "\x1b[36m",    // Cyan
                enabled: true,
            }
        } else {
            Self {
                error: "",
                warning: "",
                success: "",
                info: "",
                enabled: false,
            }
        }
    }

    pub fn reset(&self) -> &'static str {
        if self.enabled {
            RESET
        } else {
            ""
        }
    }
}

/// Per the no-color.org spec: NO_COLOR disables color only when it's set
/// *and non-empty* - some shell rc files set `NO_COLOR=` as a "reset to
/// default" no-op, which must not be read as "disable color" (issue #102).
/// Takes the raw env value (rather than reading the env itself) so this
/// decision is testable without mutating process-global env state.
fn no_color_env_disables(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

pub fn should_use_colors(force_color: bool, no_color: bool) -> bool {
    // Priority: --no-color > --color > NO_COLOR env > TTY detection
    if no_color {
        return false;
    }
    if force_color {
        return true;
    }
    // var_os (not var) so a non-UTF-8 but still non-empty value still
    // counts as "set".
    if no_color_env_disables(std::env::var_os("NO_COLOR").as_deref()) {
        return false;
    }
    io::stdout().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_102() {
        assert!(!no_color_env_disables(None), "unset must not disable color");
        assert!(
            !no_color_env_disables(Some(std::ffi::OsStr::new(""))),
            "NO_COLOR set but empty must not disable color, per the no-color.org spec"
        );
        assert!(no_color_env_disables(Some(std::ffi::OsStr::new("1"))));
        assert!(no_color_env_disables(Some(std::ffi::OsStr::new("0"))));
    }
}
