//! Strictly monochrome palette — the terminal port of the web app's design tokens.

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub muted: Color,
    pub line: Color,
    pub inverse_fg: Color,
    pub inverse_bg: Color,
}

impl Theme {
    pub fn from_name(name: &str) -> Theme {
        if name == "light" {
            Theme {
                fg: Color::Black,
                bg: Color::White,
                muted: Color::DarkGray,
                line: Color::Gray,
                inverse_fg: Color::White,
                inverse_bg: Color::Black,
            }
        } else {
            Theme {
                fg: Color::White,
                bg: Color::Black,
                muted: Color::Gray,
                line: Color::DarkGray,
                inverse_fg: Color::Black,
                inverse_bg: Color::White,
            }
        }
    }

    pub fn base(&self) -> Style {
        Style::default().fg(self.fg).bg(self.bg)
    }

    pub fn dim(&self) -> Style {
        Style::default().fg(self.muted).bg(self.bg)
    }

    pub fn faint(&self) -> Style {
        Style::default().fg(self.line).bg(self.bg)
    }

    pub fn bold(&self) -> Style {
        Style::default()
            .fg(self.fg)
            .bg(self.bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn inverse(&self) -> Style {
        Style::default()
            .fg(self.inverse_fg)
            .bg(self.inverse_bg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn border(&self) -> Style {
        Style::default().fg(self.line).bg(self.bg)
    }
}

/// Glyphs lifted from the landing page mock.
pub const PROMPT: &str = "❯";
pub const AGENT: &str = "◆";
pub const DOT_ON: &str = "●";
pub const DOT_OFF: &str = "○";
pub const STAR: &str = "★";
pub const BAR: &str = "│";
pub const HEAT: [&str; 5] = ["·", "░", "▒", "▓", "█"];
