//! Tail view — outer rounded Block with tab-strip title, Records Table,
//! tri-section middle bar (filter/levels/rate). Footer is drawn by `view_footer`.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row, Table, TableState};
use ratatui::Frame;
use tracecure_core::{Level, Record};

use super::focus::Pane;
use super::state::AppState;

use chrono::Utc;
use timeago::Formatter;

pub fn draw_tail(f: &mut Frame, area: Rect, state: &AppState) {
    let recs = state.filtered();
    let outer = outer_block(state, recs.len());
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Split into records (fills) and middle bar (3 rows, including borders).
    let [records_area, middle_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(inner);

    render_records(f, records_area, state, &recs);
    super::view_tail_middle::draw_middle(f, middle_area, state);
}

fn outer_block(state: &AppState, count: usize) -> Block<'static> {
    let tabs = Line::from(vec![
        Span::raw(" tracecure  \u{2022}  "),
        Span::styled(
            "\u{27e8} Tail \u{27e9}",
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" \u{00b7} "),
        Span::styled("Search", Style::default().fg(state.theme.muted)),
        Span::raw(" "),
    ]);

    let cap = crate::tui::state::RECORD_CAP;
    let rate = state.rate_per_sec();
    let meta = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("{count}/{cap}"),
            Style::default().fg(state.theme.muted),
        ),
        Span::styled("  \u{2022}  ", Style::default().fg(state.theme.muted)),
        Span::styled(format!("{rate}/s"), Style::default().fg(state.theme.muted)),
        Span::raw(" "),
    ]);

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style(state, Pane::Records))
        .title(tabs)
        .title(meta.right_aligned())
}

fn border_style(state: &AppState, pane: Pane) -> Style {
    if state.focus_pane == pane {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    }
}

fn render_records(f: &mut Frame, area: Rect, state: &AppState, recs: &[&Record]) {
    if recs.is_empty() {
        let msg = if state.filter.value().is_empty() {
            "waiting for records\u{2026}"
        } else {
            "no records match the filter"
        };
        f.render_widget(
            ratatui::widgets::Paragraph::new(msg).style(Style::default().fg(state.theme.muted)),
            area,
        );
        return;
    }

    let visible = area.height as usize;
    let cursor = state.tail_cursor_or_last();
    let (start, end) = window_around_cursor(recs.len(), visible.saturating_sub(1), cursor);

    let header = Row::new(vec![
        Cell::from("time"),
        Cell::from("host"),
        Cell::from("app"),
        Cell::from("lvl"),
        Cell::from("body"),
    ])
    .style(Style::default().fg(state.theme.muted))
    .bottom_margin(0);

    let rows: Vec<Row> = recs[start..end]
        .iter()
        .enumerate()
        .map(|(i, r)| row(r, start + i == cursor, state))
        .collect();

    let widths = [
        Constraint::Length(14),
        Constraint::Length(10),
        Constraint::Length(7),
        Constraint::Length(5),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(if state.focus_pane == Pane::Records {
                    state.theme.bg_selected
                } else {
                    state.theme.bg_selected_unfocused
                })
                .fg(state.theme.fg_selected),
        )
        .highlight_symbol("\u{2595} ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    let mut table_state = TableState::default().with_selected(Some(cursor - start));
    f.render_stateful_widget(table, area, &mut table_state);
}

pub(super) fn format_ts(
    ts: chrono::DateTime<chrono::Utc>,
    mode: crate::tui::state::TimestampMode,
) -> String {
    use crate::tui::state::TimestampMode;
    match mode {
        TimestampMode::Absolute => ts.format("%H:%M:%S%.3f").to_string(),
        TimestampMode::Relative => {
            let now = Utc::now();
            let diff = now.signed_duration_since(ts);
            // chrono::Duration → std::time::Duration for timeago::Formatter.
            let std_dur = if diff.num_milliseconds() >= 0 {
                std::time::Duration::from_millis(
                    u64::try_from(diff.num_milliseconds()).unwrap_or(0),
                )
            } else {
                std::time::Duration::ZERO
            };
            Formatter::new().convert(std_dur)
        }
    }
}

fn row<'a>(r: &'a Record, selected: bool, state: &AppState) -> Row<'a> {
    let ts = format_ts(r.timestamp, state.timestamp_mode);
    let level_cell = level_block_cell(r.level, state);
    let body = r.body.clone();
    let cells = vec![
        Cell::from(ts).style(Style::default().fg(state.theme.muted)),
        Cell::from(r.host.clone()).style(Style::default().fg(state.theme.fg)),
        Cell::from(r.app.clone()).style(Style::default().fg(state.theme.fg)),
        level_cell,
        Cell::from(body).style(Style::default().fg(state.theme.fg)),
    ];
    let row = Row::new(cells);
    if selected {
        row.style(Style::default().add_modifier(Modifier::BOLD))
    } else {
        row
    }
}

fn level_block_cell(lvl: Level, state: &AppState) -> Cell<'static> {
    let color = state.theme.level_color(lvl);
    let text = match lvl {
        Level::Trace => "\u{2591}\u{2591}\u{2591}",
        Level::Debug => "\u{2592}\u{2592}\u{2592}",
        _ => "\u{2588}\u{2588}\u{2588}",
    };
    Cell::from(text).style(Style::default().fg(color))
}

/// Compute the (start, end) slice of `recs` that should be visible given
/// `visible_rows` and the cursor index. Keeps the cursor on screen.
pub(super) fn window_around_cursor(
    total: usize,
    visible_rows: usize,
    cursor: usize,
) -> (usize, usize) {
    if total == 0 || visible_rows == 0 {
        return (0, 0);
    }
    let visible = visible_rows.min(total);
    let cursor = cursor.min(total.saturating_sub(1));
    let start = (cursor + 1).saturating_sub(visible);
    let end = (start + visible).min(total);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_scrolls_to_keep_cursor_visible() {
        // 100 total, 10 visible, cursor at 50 — window is 41..51.
        assert_eq!(window_around_cursor(100, 10, 50), (41, 51));
    }

    #[test]
    fn window_clamps_at_top() {
        assert_eq!(window_around_cursor(100, 10, 0), (0, 10));
    }

    #[test]
    fn window_clamps_at_end_when_cursor_past_total() {
        // 5 total, 10 visible requested — show all, cursor is clamped.
        assert_eq!(window_around_cursor(5, 10, 99), (0, 5));
    }

    #[test]
    fn window_empty_inputs_return_zero() {
        assert_eq!(window_around_cursor(0, 10, 0), (0, 0));
        assert_eq!(window_around_cursor(10, 0, 0), (0, 0));
    }
}
