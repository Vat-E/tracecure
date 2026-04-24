//! Tail-mode middle bar — three bordered blocks: filter / levels / rate.
//! Each block's border thickens + colours when that pane is focused.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Sparkline};
use ratatui::Frame;

use super::focus::Pane;
use super::state::AppState;

pub fn draw_middle(f: &mut Frame, area: Rect, state: &AppState) {
    let [filter_area, levels_area, rate_area] = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .areas(area);

    draw_filter(f, filter_area, state);
    draw_levels(f, levels_area, state);
    draw_rate(f, rate_area, state);
}

fn draw_filter(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus_pane == Pane::Filter;
    let block = section_block(" filter ", focused, state);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let marker = if focused { "▸ " } else { "  " };
    let line = Line::from(vec![
        Span::styled(marker, Style::default().fg(state.theme.accent)),
        Span::raw(state.filter.value().to_string()),
    ]);
    f.render_widget(Paragraph::new(line), inner);

    if focused {
        // Position cursor at the visual cursor position inside the input.
        let x = inner.x + 2 + u16::try_from(state.filter.visual_cursor()).unwrap_or(0);
        f.set_cursor_position((x, inner.y));
    }
}

fn draw_levels(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus_pane == Pane::LevelsBar;
    let block = section_block(" levels ", focused, state);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let set = state.level_set;
    let pills = [
        ('I', set.info, state.theme.level_info),
        ('W', set.warn, state.theme.level_warn),
        ('E', set.error, state.theme.level_error),
        ('F', set.fatal, state.theme.level_fatal),
        ('D', set.debug, state.theme.level_debug),
        ('T', set.trace, state.theme.level_trace),
    ];

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(pills.len() * 3);
    for (i, (letter, on, color)) in pills.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let dot = if *on { "●" } else { "○" };
        let letter_style = if *on {
            Style::default().fg(*color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(state.theme.disabled)
        };
        spans.push(Span::styled(
            dot.to_string(),
            Style::default().fg(if *on { *color } else { state.theme.disabled }),
        ));
        spans.push(Span::styled(letter.to_string(), letter_style));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_rate(f: &mut Frame, area: Rect, state: &AppState) {
    // Rate is display-only; it never takes focus, but we still style the
    // border per the theme.
    let block = section_block(" rate 1m ", false, state);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [spark_area, number_area] =
        Layout::horizontal([Constraint::Min(5), Constraint::Length(7)]).areas(inner);

    let data: Vec<u64> = state.rate_history.iter().map(|x| u64::from(*x)).collect();
    let sparkline = Sparkline::default()
        .data(&data)
        .style(Style::default().fg(state.theme.accent));
    f.render_widget(sparkline, spark_area);

    let rate = state.rate_per_sec();
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("{rate}/s"),
            Style::default().fg(state.theme.muted),
        )),
        number_area,
    );
}

fn section_block(title: &'static str, focused: bool, state: &AppState) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    };
    let border_type = if focused {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(title, Style::default().fg(state.theme.muted)))
}
