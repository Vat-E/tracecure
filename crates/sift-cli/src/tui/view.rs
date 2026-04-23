//! Top-level draw dispatcher. Picks a view by current mode. Overlay rendering
//! (help, detail) lands in later tasks.

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::state::{AppState, Mode};
use super::view_tail::draw_tail;

pub fn draw(f: &mut Frame, state: &AppState) {
    match state.mode {
        Mode::Tail => draw_tail(f, state),
        Mode::Search => draw_search_placeholder(f, state),
    }
}

/// Temporary placeholder. Replaced by `view_search::draw_search` in Task 6.
fn draw_search_placeholder(f: &mut Frame, _state: &AppState) {
    let area = f.area();
    let line = Line::from(vec![
        Span::styled("sift · search", Style::default().fg(Color::Gray)),
        Span::raw("   "),
        Span::styled(
            "(coming soon — press t to return to tail)",
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let [header, _] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    f.render_widget(Paragraph::new(line), header);
}
