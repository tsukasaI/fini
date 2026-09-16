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

/// The full should_use_colors decision, parameterized so it's testable
/// without mutating process-global env state (or depending on a real TTY).
/// Per the no-color.org spec: NO_COLOR disables color only when it's set
/// *and non-empty* (issue #102).
fn color_decision(
    force_color: bool,
    no_color: bool,
    no_color_env: Option<&std::ffi::OsStr>,
    is_tty: bool,
) -> bool {
    // Priority: --no-color > --color > NO_COLOR env > TTY detection
    if no_color {
        return false;
    }
    if force_color {
        return true;
    }
    if no_color_env.is_some_and(|v| !v.is_empty()) {
        return false;
    }
    is_tty
}

pub fn should_use_colors(force_color: bool, no_color: bool) -> bool {
    // var_os (not var) so a non-UTF-8 but still non-empty NO_COLOR value
    // still counts as "set".
    color_decision(
        force_color,
        no_color,
        std::env::var_os("NO_COLOR").as_deref(),
        io::stdout().is_terminal(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn test_issue_102() {
        assert!(
            color_decision(false, false, None, true),
            "unset NO_COLOR must not disable color"
        );
        assert!(
            color_decision(false, false, Some(OsStr::new("")), true),
            "NO_COLOR set but empty must not disable color, per the no-color.org spec"
        );
        assert!(!color_decision(false, false, Some(OsStr::new("1")), true));
        assert!(!color_decision(false, false, Some(OsStr::new("0")), true));

        // --color still overrides NO_COLOR entirely.
        assert!(color_decision(true, false, Some(OsStr::new("1")), false));
    }
}
