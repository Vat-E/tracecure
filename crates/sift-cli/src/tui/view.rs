//! Top-level draw dispatcher. Picks a mode view and overlays detail/help/footer.

use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;

use super::state::{AppState, Mode};
use super::view_detail::draw_detail;
use super::view_footer::draw_footer;
use super::view_help::draw_help;
use super::view_search::draw_search;
use super::view_tail::draw_tail;

pub fn draw(f: &mut Frame, state: &AppState) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

    match state.mode {
        Mode::Tail => draw_tail(f, body, state),
        Mode::Search => draw_search(f, body, state),
    }
    draw_footer(f, footer, state);

    if let Some(r) = &state.detail {
        draw_detail(f, r);
    }
    if state.help_open {
        draw_help(f);
    }
}
