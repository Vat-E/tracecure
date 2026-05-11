//! Syntect → Ratatui adapter. Renders a string as a `Vec<Line<'static>>` using
//! a named theme from `syntect::highlighting::ThemeSet::load_defaults`.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SynColor, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Highlight `source` as JSON using the given theme name. Returns None on any
/// error (unknown theme, syntax missing, etc.) so the caller can fall back.
#[must_use]
pub fn highlight_json(source: &str, theme_name: &str) -> Option<Vec<Line<'static>>> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ss.find_syntax_by_extension("json")?;
    let theme = ts.themes.get(theme_name)?;
    let mut h = HighlightLines::new(syntax, theme);

    let mut out: Vec<Line<'static>> = Vec::new();
    for line in source.lines() {
        let ranges = h.highlight_line(line, &ss).ok()?;
        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, text)| {
                Span::styled(
                    text.to_string(),
                    Style::default().fg(to_ratatui_color(style.foreground)),
                )
            })
            .collect();
        out.push(Line::from(spans));
    }
    Some(out)
}

fn to_ratatui_color(c: SynColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_trivial_json_object() {
        let lines = highlight_json("{\"n\":1}", "base16-ocean.dark");
        assert!(lines.is_some());
        let lines = lines.unwrap();
        assert!(!lines.is_empty());
    }

    #[test]
    fn unknown_theme_returns_none() {
        assert!(highlight_json("{}", "no-such-theme-12345").is_none());
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        let lines = highlight_json("", "base16-ocean.dark").unwrap();
        assert!(lines.is_empty());
    }
}
