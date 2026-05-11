//! Level bar for search mode — reuses the tail-mode `draw_levels` shape but
//! as its own block (no filter/rate neighbours).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use super::focus::Pane;
use super::state::AppState;

pub fn draw_levels(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus_pane == Pane::LevelsBar;
    let border_type = if focused {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };
    let border_style = if focused {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(
            " levels ",
            Style::default().fg(state.theme.muted),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [pills_area] = Layout::horizontal([Constraint::Min(1)]).areas(inner);

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
        spans.push(Span::styled(
            dot.to_string(),
            Style::default().fg(if *on { *color } else { state.theme.disabled }),
        ));
        spans.push(Span::styled(
            letter.to_string(),
            if *on {
                Style::default().fg(*color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(state.theme.disabled)
            },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), pills_area);
}
