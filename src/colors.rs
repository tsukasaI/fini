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

pub fn should_use_colors(force_color: bool, no_color: bool) -> bool {
    // Priority: --no-color > --color > NO_COLOR env > TTY detection
    if no_color {
        return false;
    }
    if force_color {
        return true;
    }
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    io::stdout().is_terminal()
}
