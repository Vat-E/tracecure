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

fn search(state: &mut AppState, key: KeyEvent) {
    use super::state::{SearchFocus, SearchStatus};

    match key.code {
        KeyCode::Esc => state.set_mode(Mode::Tail),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => state.quit(),
        KeyCode::Tab => state.cycle_search_focus(true),
        KeyCode::BackTab => state.cycle_search_focus(false),

        // Quick-range keys require Alt+1/2/3 because unmodified digits would
        // otherwise be typed into whichever search field currently has focus.
        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => {
            apply_quick_range(state, 3_600);
        }
        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::ALT) => {
            apply_quick_range(state, 86_400);
        }
        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::ALT) => {
            apply_quick_range(state, 7 * 86_400);
        }

        KeyCode::Up => state.search_cursor_up(),
        KeyCode::Down => state.search_cursor_down(),

        KeyCode::Enter => {
            // Enter on the Query field submits. Enter on From/To cycles focus.
            match state.search_focus {
                SearchFocus::Query => state.search_status = SearchStatus::Running,
                _ => state.cycle_search_focus(true),
            }
        }

        KeyCode::Backspace => pop_focused(state),
        KeyCode::Char(c) => push_focused(state, c),

        _ => {}
    }
}

fn push_focused(state: &mut AppState, c: char) {
    use super::state::SearchFocus;
    match state.search_focus {
        SearchFocus::Query => state.search_q.push(c),
        SearchFocus::From => state.search_from.push(c),
        SearchFocus::To => state.search_to.push(c),
    }
}

fn pop_focused(state: &mut AppState) {
    use super::state::SearchFocus;
    match state.search_focus {
        SearchFocus::Query => {
            state.search_q.pop();
        }
        SearchFocus::From => {
            state.search_from.pop();
        }
        SearchFocus::To => {
            state.search_to.pop();
        }
    }
}

fn apply_quick_range(state: &mut super::state::AppState, seconds_back: i64) {
    use chrono::Utc;
    let now = Utc::now();
    let lo = now - chrono::Duration::seconds(seconds_back);
    state.search_from = lo.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    state.search_to = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    state.search_status = super::state::SearchStatus::Running;
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
    fn esc_in_search_switches_to_tail() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Esc));
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

    #[test]
    fn tab_in_search_mode_cycles_focus() {
        use super::super::state::{Mode, SearchFocus};
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Tab));
        assert_eq!(s.search_focus, SearchFocus::From);
        dispatch(&mut s, k(KeyCode::Tab));
        assert_eq!(s.search_focus, SearchFocus::To);
    }

    #[test]
    fn typing_in_search_query_appends_to_search_q() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Char('a')));
        dispatch(&mut s, k(KeyCode::Char('b')));
        assert_eq!(s.search_q, "ab");
    }

    #[test]
    fn enter_in_query_marks_status_running() {
        use super::super::state::{Mode, SearchStatus};
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        s.search_q = "boom".into();
        dispatch(&mut s, k(KeyCode::Enter));
        assert!(matches!(s.search_status, SearchStatus::Running));
    }

    #[test]
    fn backspace_in_focused_field_pops_char() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        s.search_q = "abc".into();
        dispatch(&mut s, k(KeyCode::Backspace));
        assert_eq!(s.search_q, "ab");
    }
}
