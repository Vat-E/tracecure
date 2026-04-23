//! Centered popup listing keybindings for the current mode.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub fn draw_help(f: &mut Frame) {
    let area = centered_rect(70, 80, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" keybindings · Esc or ? to close ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines: Vec<Line<'static>> = vec![
        section("global"),
        binding("q", "quit (in tail list)"),
        binding("Ctrl+C", "quit (any mode)"),
        binding("?", "toggle this help"),
        binding("Esc", "close overlay / return to tail / close detail"),
        binding("t", "switch to tail mode"),
        binding("s", "switch to search mode"),
        Line::raw(""),
        section("tail"),
        binding("/", "focus filter (then type; Esc/Enter to exit)"),
        binding("Up / Down", "move selection"),
        binding("Enter", "open detail popup"),
        binding("Space", "pause / resume (flushes pending)"),
        binding("Home", "scroll to top"),
        binding("End", "follow tail"),
        binding("I/W/E/F", "toggle info/warn/error/fatal visibility"),
        binding("D/T", "toggle debug/trace visibility"),
        Line::raw(""),
        section("search"),
        binding("Tab / Shift+Tab", "cycle focus (query → from → to)"),
        binding("Enter (query)", "submit search"),
        binding("Up / Down", "move result cursor"),
        binding("o", "open detail popup for selected result"),
        binding("Alt+1 / Alt+2 / Alt+3", "quick range: 1h / 24h / 7d"),
        binding("I/W/E/F/D/T", "toggle level visibility"),
        Line::raw(""),
        section("detail"),
        binding("Esc", "close"),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))
}

fn binding(key: &'static str, desc: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{key:16}"), Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::raw(desc),
    ])
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
