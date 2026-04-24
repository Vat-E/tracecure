//! Tail view. Moved from view.rs during UX v2 scaffolding.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;
use sift_core::{Level, Record};

use super::state::{AppState, Focus, ScrollMode, Status};

pub fn draw_tail(f: &mut Frame, area: Rect, state: &AppState) {
    let [header, body, filter] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(f, header, state);
    render_body(f, body, state);
    render_filter(f, filter, state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let title = Span::styled(
        "sift · tail",
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    let status_span = Span::styled(
        format!(" [{}] ", state.status.label()),
        status_style(state.status),
    );
    let levels = level_indicator(state);
    let line = Line::from(vec![
        title,
        Span::raw("   "),
        status_span,
        Span::raw("   "),
        levels,
    ]);
    f.render_widget(Paragraph::new(line), area);
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

fn status_style(s: Status) -> Style {
    match s {
        Status::Connected => Style::default().fg(Color::Green),
        Status::Disconnected => Style::default().fg(Color::Red),
        Status::Connecting | Status::Reconnecting { .. } => Style::default().fg(Color::Yellow),
    }
}

fn render_body(f: &mut Frame, area: Rect, state: &AppState) {
    let recs = state.filtered();
    if recs.is_empty() {
        let msg = if state.filter.value().is_empty() {
            "waiting for records\u{2026}"
        } else {
            "no records match the filter"
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }
    let visible = area.height as usize;
    let (start, end) = visible_range(recs.len(), visible, state.scroll);
    let cursor = state.tail_cursor_or_last();
    let items: Vec<ListItem> = recs[start..end]
        .iter()
        .enumerate()
        .map(|(i, r)| row(r, start + i == cursor))
        .collect();
    f.render_widget(List::new(items), area);
}

pub(super) fn visible_range(total: usize, visible: usize, scroll: ScrollMode) -> (usize, usize) {
    if total == 0 || visible == 0 {
        return (0, 0);
    }
    let v = visible.min(total);
    match scroll {
        ScrollMode::Follow => (total - v, total),
        ScrollMode::Locked { offset } => {
            let start = offset.min(total.saturating_sub(1));
            let end = (start + v).min(total);
            (start, end)
        }
    }
}

fn row(r: &Record, selected: bool) -> ListItem<'_> {
    let ts = r.timestamp.format("%H:%M:%S%.3f").to_string();
    let level_s = level_span(r.level);
    let mut style = Style::default();
    if selected {
        style = style.bg(Color::DarkGray);
    }
    let line = Line::from(vec![
        Span::styled(ts, Style::default().fg(Color::DarkGray)),
        Span::raw(" "),
        Span::styled(format!("{:<10}", r.host), Style::default().fg(Color::Gray)),
        Span::raw(" "),
        Span::styled(format!("{:<8}", r.app), Style::default().fg(Color::Gray)),
        Span::raw(" "),
        level_s,
        Span::raw(" "),
        Span::raw(r.body.clone()),
    ])
    .style(style);
    ListItem::new(line)
}

fn level_span(lvl: Level) -> Span<'static> {
    let (text, color) = match lvl {
        Level::Trace => ("trace", Color::DarkGray),
        Level::Debug => ("debug", Color::Cyan),
        Level::Info => ("info ", Color::Green),
        Level::Warn => ("warn ", Color::Yellow),
        Level::Error => ("error", Color::Red),
        Level::Fatal => ("fatal", Color::Magenta),
    };
    Span::styled(text, Style::default().fg(color))
}

fn render_filter(f: &mut Frame, area: Rect, state: &AppState) {
    let prefix = match state.focus {
        Focus::Filter => "/",
        Focus::List => " ",
    };
    let pause_hint = if state.paused {
        Span::styled(
            format!(" [paused: {} new]", state.pending_count()),
            Style::default().fg(Color::Yellow),
        )
    } else {
        Span::raw("")
    };
    let content = if state.filter.value().is_empty() && state.focus != Focus::Filter {
        Span::styled(
            "/ filter · space pause · s search · ? help · q quit",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        Span::raw(state.filter.value())
    };
    // Matches counter: only when filter is non-empty. Shows how the substring
    // filter narrowed the level-filtered set.
    let matches = if state.filter.value().is_empty() {
        Span::raw("")
    } else {
        let shown = state.filtered().len();
        let unfiltered_by_level: usize = state
            .records_iter()
            .filter(|r| state.level_set.allows(r.level))
            .count();
        let hidden = unfiltered_by_level.saturating_sub(shown);
        Span::styled(
            format!("  {shown} matches · {hidden} filtered out"),
            Style::default().fg(Color::DarkGray),
        )
    };
    let line = Line::from(vec![
        Span::styled(prefix, Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        content,
        matches,
        pause_hint,
    ]);
    f.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::TOP)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_follow_shows_tail() {
        assert_eq!(visible_range(10, 3, ScrollMode::Follow), (7, 10));
    }

    #[test]
    fn visible_range_follow_with_fewer_records_than_rows() {
        assert_eq!(visible_range(2, 10, ScrollMode::Follow), (0, 2));
    }

    #[test]
    fn visible_range_locked_clamps_within_bounds() {
        assert_eq!(
            visible_range(10, 3, ScrollMode::Locked { offset: 4 }),
            (4, 7)
        );
        assert_eq!(
            visible_range(10, 3, ScrollMode::Locked { offset: 8 }),
            (8, 10)
        );
    }

    #[test]
    fn visible_range_empty_returns_zero() {
        assert_eq!(visible_range(0, 10, ScrollMode::Follow), (0, 0));
        assert_eq!(
            visible_range(5, 0, ScrollMode::Locked { offset: 2 }),
            (0, 0)
        );
    }
}
