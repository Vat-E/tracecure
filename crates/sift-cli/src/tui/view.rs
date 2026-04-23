//! Top-level draw dispatcher.

use ratatui::Frame;

use super::state::{AppState, Mode};
use super::view_search::draw_search;
use super::view_tail::draw_tail;

pub fn draw(f: &mut Frame, state: &AppState) {
    match state.mode {
        Mode::Tail => draw_tail(f, state),
        Mode::Search => draw_search(f, state),
    }
}
