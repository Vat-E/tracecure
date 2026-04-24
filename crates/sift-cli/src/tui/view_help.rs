//! Help overlay — two-column keybind grid generated from one constant.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Single source of truth for the help overlay. Sections in rendering order.
pub(super) const KEYBIND_TABLE: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("Ctrl+C", "quit"),
            ("?", "toggle help"),
            ("Esc", "close overlay / return to previous focus"),
            (":", "open command prompt"),
            ("Tab / Shift+Tab", "cycle focused pane"),
        ],
    ),
    (
        "Tail · Records",
        &[
            ("j / ↓", "cursor down"),
            ("k / ↑", "cursor up"),
            ("g / Home", "top"),
            ("G / End", "follow tail"),
            ("↵", "open detail"),
            ("Space", "pause / resume"),
            ("Ctrl+T", "toggle absolute / relative timestamps"),
            ("I W E F", "toggle Info / Warn / Error / Fatal"),
            ("D T", "toggle Debug / Trace"),
            ("/", "focus filter"),
            ("q", "quit"),
            ("s", "enter Search"),
            ("r", "force reconnect"),
        ],
    ),
    (
        "Search",
        &[
            ("t", "back to tail"),
            (
                "Tab / Shift+Tab",
                "cycle Query → From → To → Results → Levels",
            ),
            ("↵ (in Query)", "submit"),
            ("Alt+1 / 2 / 3", "quick range 1h / 24h / 7d"),
            ("o", "open detail for selected result"),
        ],
    ),
    (
        "Command mode",
        &[
            (":pause / :resume / :clear / :retry", ""),
            (":save path=<file>", ""),
            (":search q=<str> from=<iso> to=<iso>", ""),
            (":level <name> on|off", ""),
            (":help / :quit", ""),
            ("Tab", "complete"),
            ("Esc", "cancel"),
        ],
    ),
];

pub fn draw_help(f: &mut Frame) {
    let theme = super::theme::Theme::dark();
    let area = centered_rect(70, 90, f.area());
    f.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            " keybindings · Esc or ? to close ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (title, entries) in KEYBIND_TABLE {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            (*title).to_string(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        for (k, v) in *entries {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(format!("{k:<28}"), Style::default().fg(theme.shortcut_key)),
                Span::raw("  "),
                Span::styled((*v).to_string(), Style::default().fg(theme.shortcut_label)),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [_, h, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(v);

    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybind_table_has_every_section() {
        let titles: Vec<&str> = KEYBIND_TABLE.iter().map(|(t, _)| *t).collect();
        assert!(titles.contains(&"Global"));
        assert!(titles.contains(&"Tail · Records"));
        assert!(titles.contains(&"Search"));
        assert!(titles.contains(&"Command mode"));
    }

    #[test]
    fn every_section_has_at_least_one_entry() {
        for (title, entries) in KEYBIND_TABLE {
            assert!(!entries.is_empty(), "section '{title}' is empty");
        }
    }

    #[test]
    fn global_section_documents_ctrl_c_and_question_mark() {
        let (_, global) = KEYBIND_TABLE
            .iter()
            .find(|(t, _)| *t == "Global")
            .expect("Global section exists");
        let keys: Vec<&str> = global.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Ctrl+C"));
        assert!(keys.contains(&"?"));
        assert!(keys.contains(&":"));
    }
}
