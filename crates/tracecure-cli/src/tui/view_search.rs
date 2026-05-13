//! Search view — three bordered inputs (query/from/to), optional throbber
//! during inflight, Table of results, level bar, and the status line.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState};
use ratatui::Frame;
use throbber_widgets_tui::{Throbber, ThrobberState, BRAILLE_SIX};
use tracecure_core::{Level, Record};

use super::focus::Pane;
use super::state::{AppState, SearchFocus, SearchStatus};

pub fn draw_search(f: &mut Frame, area: Rect, state: &AppState) {
    // Compute filtered results once; pass to helpers that need it.
    let recs = state.filtered_search();

    let outer = outer_block(state, &recs);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let [query_area, fromto_area, status_area, results_area, levels_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .areas(inner);

    draw_input_block(
        f,
        query_area,
        " query ",
        state.search_q.value(),
        state.search_focus == SearchFocus::Query,
        state.focus_pane == Pane::SearchQuery,
        state,
    );

    let [from_area, to_area] =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(fromto_area);

    draw_input_block(
        f,
        from_area,
        " from ",
        state.search_from.value(),
        state.search_focus == SearchFocus::From,
        state.focus_pane == Pane::SearchFrom,
        state,
    );
    draw_input_block(
        f,
        to_area,
        " to ",
        state.search_to.value(),
        state.search_focus == SearchFocus::To,
        state.focus_pane == Pane::SearchTo,
        state,
    );

    draw_status(f, status_area, state);
    draw_results(f, results_area, state, &recs);
    super::view_search_levels::draw_levels(f, levels_area, state);
}

fn outer_block<'a>(state: &AppState, recs: &[&Record]) -> Block<'a> {
    let tabs = Line::from(vec![
        Span::raw(" tracecure  •  "),
        Span::styled("Tail", Style::default().fg(state.theme.muted)),
        Span::raw(" · "),
        Span::styled(
            "⟨ Search ⟩",
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);

    let meta = Line::from(match &state.search_status {
        SearchStatus::Ok {
            scanned,
            elapsed_ms,
        } => vec![
            Span::raw(" "),
            Span::styled(
                format!("{} results", recs.len()),
                Style::default().fg(state.theme.muted),
            ),
            Span::styled("  •  ", Style::default().fg(state.theme.muted)),
            Span::styled(
                format!("scanned {scanned}"),
                Style::default().fg(state.theme.muted),
            ),
            Span::styled("  •  ", Style::default().fg(state.theme.muted)),
            Span::styled(
                format!("{elapsed_ms} ms"),
                Style::default().fg(state.theme.muted),
            ),
            Span::raw(" "),
        ],
        _ => vec![Span::raw(" ")],
    });

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(state.theme.border))
        .title(tabs)
        .title(meta.right_aligned())
}

fn draw_input_block(
    f: &mut Frame,
    area: Rect,
    title: &'static str,
    value: &str,
    search_focus_match: bool,
    pane_focus: bool,
    state: &AppState,
) {
    let is_focused = search_focus_match || pane_focus;
    let border_type = if is_focused {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };
    let border_style = if is_focused {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(title, Style::default().fg(state.theme.muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let marker = if is_focused { "▸ " } else { "  " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(marker, Style::default().fg(state.theme.accent)),
            Span::raw(value.to_string()),
        ])),
        inner,
    );
}

fn draw_status(f: &mut Frame, area: Rect, state: &AppState) {
    let line = match &state.search_status {
        SearchStatus::Running => {
            // ThrobberState is created fresh each frame — the throbber shows a
            // static step (non-animating). Proper animation would require
            // storing ThrobberState in AppState and incrementing it on each
            // redraw tick. Deferred to a polish pass.
            let mut throbber_state = ThrobberState::default();
            throbber_state.calc_next();
            let throbber = Throbber::default()
                .label("searching…")
                .style(Style::default().fg(state.theme.accent))
                .throbber_set(BRAILLE_SIX);
            f.render_stateful_widget(throbber, area, &mut throbber_state);
            return;
        }
        // Ok: meta already shown in the outer block title.
        SearchStatus::Idle | SearchStatus::Ok { .. } => Line::from(""),
        SearchStatus::Err(e) => Line::from(vec![Span::styled(
            format!("error: {e}"),
            Style::default().fg(state.theme.flash_err),
        )]),
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_results(f: &mut Frame, area: Rect, state: &AppState, recs: &[&Record]) {
    if recs.is_empty() {
        let msg = match state.search_status {
            SearchStatus::Idle => "enter a query and press Enter",
            SearchStatus::Ok { .. } => "no results",
            SearchStatus::Running | SearchStatus::Err(_) => "",
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(state.theme.muted)),
            area,
        );
        return;
    }

    let visible = (area.height as usize).saturating_sub(1).max(1);
    let cursor = state.search_cursor.min(recs.len().saturating_sub(1));
    let (start, end) = crate::tui::view_tail::window_around_cursor(recs.len(), visible, cursor);

    let header = Row::new(vec![
        Cell::from("time"),
        Cell::from("host"),
        Cell::from("app"),
        Cell::from("lvl"),
        Cell::from("body"),
    ])
    .style(Style::default().fg(state.theme.muted));

    let rows: Vec<Row> = recs[start..end]
        .iter()
        .enumerate()
        .map(|(i, r)| row(r, start + i == cursor, state))
        .collect();

    let widths = [
        Constraint::Length(19),
        Constraint::Length(10),
        Constraint::Length(7),
        Constraint::Length(5),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(if state.focus_pane == Pane::SearchResults {
                    state.theme.bg_selected
                } else {
                    state.theme.bg_selected_unfocused
                })
                .fg(state.theme.fg_selected),
        )
        .highlight_symbol("▕ ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    // Use render_stateful_widget so row_highlight_style and highlight_symbol
    // actually render (T10 lesson: render_widget ignores highlight state).
    let mut table_state = TableState::default().with_selected(Some(cursor - start));
    f.render_stateful_widget(table, area, &mut table_state);
}

fn row<'a>(r: &'a Record, _selected: bool, state: &AppState) -> Row<'a> {
    let ts = match state.timestamp_mode {
        crate::tui::state::TimestampMode::Absolute => {
            r.timestamp.format("%Y-%m-%d %H:%M:%S").to_string()
        }
        crate::tui::state::TimestampMode::Relative => {
            crate::tui::view_tail::format_ts(r.timestamp, state.timestamp_mode)
        }
    };
    let level_cell = level_cell(r.level, state);
    Row::new(vec![
        Cell::from(ts).style(Style::default().fg(state.theme.muted)),
        Cell::from(r.host.clone()),
        Cell::from(r.app.clone()),
        level_cell,
        Cell::from(r.body.clone()),
    ])
}

fn level_cell(lvl: Level, state: &AppState) -> Cell<'static> {
    let color = state.theme.level_color(lvl);
    let text = match lvl {
        Level::Trace => "░░░",
        Level::Debug => "▒▒▒",
        _ => "███",
    };
    Cell::from(text).style(Style::default().fg(color))
}
