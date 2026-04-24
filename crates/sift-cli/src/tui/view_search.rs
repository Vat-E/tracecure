//! Search view. Three stacked inputs on top, results list below,
//! status line at the bottom.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;
use sift_core::Record;

use super::state::{AppState, SearchFocus, SearchStatus};

pub fn draw_search(f: &mut Frame, area: Rect, state: &AppState) {
    let [header, q_row, from_row, to_row, status_row, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);

    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "sift · search",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(
                format!("[{}]", state.status.label()),
                Style::default().fg(Color::Gray),
            ),
            Span::raw("   "),
            level_indicator(state),
            Span::raw("   "),
            Span::styled(
                "Tab: next · Alt+1/2/3: range · Enter: search · o: detail · Esc: tail",
                Style::default().fg(Color::DarkGray),
            ),
        ])),
        header,
    );

    render_field(
        f,
        q_row,
        "q   ",
        &state.search_q,
        state.search_focus == SearchFocus::Query,
    );
    render_field(
        f,
        from_row,
        "from",
        &state.search_from,
        state.search_focus == SearchFocus::From,
    );
    render_field(
        f,
        to_row,
        "to  ",
        &state.search_to,
        state.search_focus == SearchFocus::To,
    );

    let status_line = match &state.search_status {
        SearchStatus::Idle => String::new(),
        SearchStatus::Running => String::from("searching\u{2026}"),
        SearchStatus::Ok {
            scanned,
            elapsed_ms,
        } => format!(
            "{} results · scanned {} · {} ms",
            state.search_results.len(),
            scanned,
            elapsed_ms
        ),
        SearchStatus::Err(e) => format!("error: {e}"),
    };
    f.render_widget(
        Paragraph::new(status_line).style(Style::default().fg(Color::DarkGray)),
        status_row,
    );

    render_results(f, body, state);
}

fn render_field(f: &mut Frame, area: Rect, label: &'static str, value: &str, focused: bool) {
    let marker = if focused { "\u{25B6}" } else { " " };
    let label_style = if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(marker, Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::styled(label, label_style),
            Span::raw("  "),
            Span::raw(value.to_string()),
        ])),
        area,
    );
}

fn render_results(f: &mut Frame, area: Rect, state: &AppState) {
    let recs = state.filtered_search();
    if recs.is_empty() {
        let msg = match &state.search_status {
            SearchStatus::Idle => "enter a query and press Enter",
            SearchStatus::Running => "searching\u{2026}",
            SearchStatus::Err(_) => "",
            SearchStatus::Ok { .. } => "no results",
        };
        f.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::TOP)),
            area,
        );
        return;
    }

    let visible = (area.height as usize).saturating_sub(1).max(1);
    let total = recs.len();
    let cursor = state.search_cursor.min(total - 1);

    let start = if cursor >= visible {
        cursor + 1 - visible
    } else {
        0
    };
    let end = (start + visible).min(total);

    let items: Vec<ListItem> = recs[start..end]
        .iter()
        .enumerate()
        .map(|(i, r)| row(r, start + i == cursor))
        .collect();

    f.render_widget(
        List::new(items).block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn level_indicator(state: &AppState) -> Span<'static> {
    let set = state.level_set;
    let chars = [
        ('T', set.trace),
        ('D', set.debug),
        ('I', set.info),
        ('W', set.warn),
        ('E', set.error),
        ('F', set.fatal),
    ];
    let mut out = String::from("[");
    for (c, on) in chars {
        out.push(if on { c } else { '.' });
    }
    out.push(']');
    Span::styled(out, Style::default().fg(Color::DarkGray))
}

fn row(r: &Record, selected: bool) -> ListItem<'_> {
    let bg = if selected {
        Some(Color::DarkGray)
    } else {
        None
    };
    let style = bg.map(|c| Style::default().bg(c)).unwrap_or_default();

    let ts = r.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
    let line = Line::from(vec![
        Span::styled(ts, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::raw(format!("{:<10}", r.host)),
        Span::raw(" "),
        Span::raw(format!("{:<8}", r.app)),
        Span::raw(" "),
        Span::raw(format!("{:?}", r.level)),
        Span::raw(" "),
        Span::raw(r.body.clone()),
    ])
    .style(style);
    ListItem::new(line)
}
