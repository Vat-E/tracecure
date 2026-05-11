//! Theme — the single source of truth for every colour the TUI renders.
//!
//! One `Theme` instance lives in `AppState`. Views read slots off it rather
//! than hardcoding `Color::Cyan` etc., so re-skinning is a one-file change.

use ratatui::style::Color;
use sift_core::Level;

#[derive(Clone, Debug)]
#[allow(dead_code, clippy::struct_field_names)]
pub struct Theme {
    // Base
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,

    // Borders
    pub border: Color,
    pub border_focused: Color,

    // Selection
    pub bg_selected: Color,
    pub fg_selected: Color,
    pub bg_selected_unfocused: Color,

    // Levels
    pub level_trace: Color,
    pub level_debug: Color,
    pub level_info: Color,
    pub level_warn: Color,
    pub level_error: Color,
    pub level_fatal: Color,

    // Semantic
    pub shortcut_key: Color,
    pub shortcut_label: Color,
    pub flash_ok: Color,
    pub flash_err: Color,
    pub disabled: Color,

    // Syntect integration
    pub highlighter_theme: &'static str,
}

impl Theme {
    #[must_use]
    pub fn dark() -> Self {
        Self {
            fg: Color::Rgb(215, 215, 215),
            muted: Color::Rgb(120, 120, 120),
            accent: Color::Cyan,

            border: Color::Rgb(60, 60, 60),
            border_focused: Color::Cyan,

            bg_selected: Color::Rgb(50, 50, 80),
            fg_selected: Color::Rgb(255, 255, 255),
            bg_selected_unfocused: Color::Rgb(40, 40, 50),

            level_trace: Color::DarkGray,
            level_debug: Color::Cyan,
            level_info: Color::Green,
            level_warn: Color::Yellow,
            level_error: Color::Red,
            level_fatal: Color::Magenta,

            shortcut_key: Color::Cyan,
            shortcut_label: Color::Gray,
            flash_ok: Color::Green,
            flash_err: Color::Red,
            disabled: Color::DarkGray,

            highlighter_theme: "base16-ocean.dark",
        }
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn level_color(&self, lvl: Level) -> Color {
        match lvl {
            Level::Trace => self.level_trace,
            Level::Debug => self.level_debug,
            Level::Info => self.level_info,
            Level::Warn => self.level_warn,
            Level::Error => self.level_error,
            Level::Fatal => self.level_fatal,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_default_has_every_slot_filled() {
        let t = Theme::dark();
        // Smoke-check a handful of slots to ensure they're not zero-initialised.
        assert_eq!(t.accent, Color::Cyan);
        assert_eq!(t.border_focused, Color::Cyan);
        assert_eq!(t.level_error, Color::Red);
        assert_eq!(t.flash_ok, Color::Green);
        assert_eq!(t.highlighter_theme, "base16-ocean.dark");
    }

    #[test]
    fn level_color_maps_every_level_to_its_slot() {
        let t = Theme::dark();
        assert_eq!(t.level_color(Level::Trace), t.level_trace);
        assert_eq!(t.level_color(Level::Debug), t.level_debug);
        assert_eq!(t.level_color(Level::Info), t.level_info);
        assert_eq!(t.level_color(Level::Warn), t.level_warn);
        assert_eq!(t.level_color(Level::Error), t.level_error);
        assert_eq!(t.level_color(Level::Fatal), t.level_fatal);
    }

    #[test]
    fn default_equals_dark() {
        // `Theme::default()` is just a sugared `dark()` — keep them aligned.
        let a = Theme::default();
        let b = Theme::dark();
        assert_eq!(a.accent, b.accent);
        assert_eq!(a.level_info, b.level_info);
    }
}
