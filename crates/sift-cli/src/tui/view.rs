//! Top-level draw dispatcher.

use ratatui::Frame;

use super::state::{AppState, Mode};
use super::view_detail::draw_detail;
use super::view_help::draw_help;
use super::view_search::draw_search;
use super::view_tail::draw_tail;

pub fn draw(f: &mut Frame, state: &AppState) {
    match state.mode {
        Mode::Tail => draw_tail(f, state),
        Mode::Search => draw_search(f, state),
    }
    if let Some(r) = &state.detail {
        draw_detail(f, r);
    }
    if state.help_open {
        draw_help(f);
    }
}
