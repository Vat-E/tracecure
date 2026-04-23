//! Keyboard dispatch. Pure function: `AppState` in, mutations out. The
//! `mod.rs` caller converts crossterm events into `KeyEvent`s and drops them
//! here. Keep this file free of I/O — unit tests drive it directly.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::{AppState, Focus, Mode};

pub fn dispatch(state: &mut AppState, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        state.quit();
        return;
    }

    match (state.mode, state.focus) {
        (Mode::Tail, Focus::List) => tail_list(state, key),
        (Mode::Tail, Focus::Filter) => tail_filter(state, key),
        (Mode::Search, _) => search(state, key),
    }
}

fn tail_list(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('/') => state.focus_filter(),
        KeyCode::Char('s') => state.set_mode(Mode::Search),
        KeyCode::Char('t') => state.set_mode(Mode::Tail),
        KeyCode::Up => state.scroll_up(1),
        KeyCode::Down => state.scroll_down(1),
        KeyCode::PageUp => state.scroll_up(10),
        KeyCode::PageDown => state.scroll_down(10),
        KeyCode::Home => state.scroll_up(usize::MAX / 2),
        KeyCode::End => state.follow(),
        _ => {}
    }
}

fn tail_filter(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => state.unfocus_filter(),
        KeyCode::Backspace => {
            let mut s = state.filter.clone();
            s.pop();
            state.set_filter(s);
        }
        KeyCode::Char(c) => {
            let mut s = state.filter.clone();
            s.push(c);
            state.set_filter(s);
        }
        _ => {}
    }
}

/// Stub for Task 6 — switching back to tail is the only Search-mode key for now.
fn search(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('t') => state.set_mode(Mode::Tail),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn k(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn k_ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::CONTROL,
            kind: crossterm::event::KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn ctrl_c_quits_from_any_mode() {
        let mut s = AppState::new();
        dispatch(&mut s, k_ctrl(KeyCode::Char('c')));
        assert!(s.should_quit);

        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k_ctrl(KeyCode::Char('c')));
        assert!(s.should_quit);
    }

    #[test]
    fn s_key_in_tail_switches_to_search() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('s')));
        assert_eq!(s.mode, Mode::Search);
    }

    #[test]
    fn t_key_in_search_switches_to_tail() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Char('t')));
        assert_eq!(s.mode, Mode::Tail);
    }

    #[test]
    fn slash_in_tail_list_focuses_filter() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('/')));
        assert_eq!(s.focus, Focus::Filter);
    }

    #[test]
    fn q_in_tail_list_quits() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('q')));
        assert!(s.should_quit);
    }
}
