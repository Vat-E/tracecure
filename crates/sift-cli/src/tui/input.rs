//! Keyboard dispatch. Pure function: `AppState` in, mutations out. The
//! `mod.rs` caller converts crossterm events into `KeyEvent`s and drops them
//! here. Keep this file free of I/O — unit tests drive it directly.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;
use tui_input::backend::crossterm::EventHandler;

use super::state::{AppState, Focus, Mode};

pub fn dispatch(state: &mut AppState, key: KeyEvent, retry_tx: &mpsc::Sender<()>) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        state.quit();
        return;
    }

    // Command prompt takes precedence over everything except Ctrl+C.
    if state.command_open {
        command_prompt(state, key);
        return;
    }

    // Help overlay swallows keys except Esc and ?.
    if state.help_open {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => state.toggle_help(),
            _ => {}
        }
        return;
    }

    // Detail popup swallows keys except Esc.
    if state.detail.is_some() {
        if key.code == KeyCode::Esc {
            state.close_detail();
        }
        return;
    }

    // Global ? toggles help unless we're editing text: tail-mode filter focus,
    // or any field in search mode (which is always editing one of three fields).
    let editing_text = matches!(
        (state.mode, state.focus),
        (Mode::Tail, Focus::Filter) | (Mode::Search, _)
    );
    if key.code == KeyCode::Char('?') && !editing_text {
        state.toggle_help();
        return;
    }

    // `:` opens command prompt outside text-editing contexts.
    if key.code == KeyCode::Char(':') && !editing_text {
        state.open_command_prompt();
        return;
    }

    // Global 'r' triggers immediate reconnect attempt (tail list only).
    if key.code == KeyCode::Char('r') && state.mode == Mode::Tail && state.focus == Focus::List {
        let _ = retry_tx.try_send(());
        return;
    }

    match (state.mode, state.focus) {
        (Mode::Tail, Focus::List) => tail_list(state, key),
        (Mode::Tail, Focus::Filter) => tail_filter(state, key),
        (Mode::Search, _) => search(state, key),
    }
}

fn command_prompt(state: &mut AppState, key: KeyEvent) {
    use crate::tui::command::{complete, parse};

    match key.code {
        KeyCode::Esc => {
            state.close_command_prompt();
        }
        KeyCode::Enter => {
            let input = state.command_input.value().to_string();
            match parse(&input) {
                Ok(action) => {
                    state.pending_command = Some(action);
                    state.close_command_prompt();
                }
                Err(err) => {
                    state.set_flash_err(format!("bad command: {err}"));
                    // Keep the prompt open so the user can edit.
                }
            }
        }
        KeyCode::Tab => {
            let completed = complete(state.command_input.value()).prefix;
            if !completed.is_empty() {
                state.command_input = tui_input::Input::new(completed);
            }
        }
        _ => {
            // Route every other key through tui-input so it handles cursor
            // movement, backspace, typed chars, etc.
            let evt = crossterm::event::Event::Key(key);
            state.command_input.handle_event(&evt);
        }
    }
}

fn tail_list(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('/') => state.focus_filter(),
        KeyCode::Char('s') => state.set_mode(Mode::Search),
        KeyCode::Char('t') => state.set_mode(Mode::Tail),
        KeyCode::Up => {
            state.scroll_up(1);
            state.tail_cursor_up();
        }
        KeyCode::Down => {
            state.scroll_down(1);
            state.tail_cursor_down();
        }
        KeyCode::PageUp => state.scroll_up(10),
        KeyCode::PageDown => state.scroll_down(10),
        KeyCode::Home => state.scroll_up(usize::MAX / 2),
        KeyCode::End => {
            state.follow();
            state.tail_cursor_follow();
        }
        KeyCode::Enter => {
            let idx = state.tail_cursor_or_last();
            let recs = state.filtered();
            if let Some(r) = recs.get(idx) {
                let cloned = (*r).clone();
                state.open_detail(cloned);
            }
        }
        KeyCode::Char('I') => state.toggle_level(sift_core::Level::Info),
        KeyCode::Char('W') => state.toggle_level(sift_core::Level::Warn),
        KeyCode::Char('E') => state.toggle_level(sift_core::Level::Error),
        KeyCode::Char('F') => state.toggle_level(sift_core::Level::Fatal),
        KeyCode::Char('D') => state.toggle_level(sift_core::Level::Debug),
        KeyCode::Char('T') => state.toggle_level(sift_core::Level::Trace),
        KeyCode::Char(' ') => state.toggle_pause(),
        _ => {}
    }
}

fn tail_filter(state: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => state.unfocus_filter(),
        _ => {
            crate::tui::record_input::feed(&mut state.filter, key);
            // Reset scroll to follow when filter changes (preserve the
            // existing behaviour from set_filter).
            state.scroll = crate::tui::state::ScrollMode::Follow;
        }
    }
}

fn search(state: &mut AppState, key: KeyEvent) {
    use super::state::{SearchFocus, SearchStatus};

    match key.code {
        KeyCode::Esc => state.set_mode(Mode::Tail),
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

        KeyCode::Char('o') if !state.search_results.is_empty() => {
            if let Some(r) = state.search_results.get(state.search_cursor) {
                let cloned = r.clone();
                state.open_detail(cloned);
            }
        }

        KeyCode::Char('I') => state.toggle_level(sift_core::Level::Info),
        KeyCode::Char('W') => state.toggle_level(sift_core::Level::Warn),
        KeyCode::Char('E') => state.toggle_level(sift_core::Level::Error),
        KeyCode::Char('F') => state.toggle_level(sift_core::Level::Fatal),
        KeyCode::Char('D') => state.toggle_level(sift_core::Level::Debug),
        KeyCode::Char('T') => state.toggle_level(sift_core::Level::Trace),

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
    // Known limitation: pressing Alt+1/2/3 while a search is already Running
    // updates search_from/search_to but does NOT re-fire the HTTP task until
    // the in-flight request completes. The `last_seen_running` guard in
    // `mod.rs` suppresses the second fire. A proper fix needs request-ID
    // supersession; deferred until Task 10 or later.
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

    fn dummy_retry() -> mpsc::Sender<()> {
        let (tx, _rx) = mpsc::channel::<()>(1);
        tx
    }

    #[test]
    fn ctrl_c_quits_from_any_mode() {
        let mut s = AppState::new();
        dispatch(&mut s, k_ctrl(KeyCode::Char('c')), &dummy_retry());
        assert!(s.should_quit);

        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k_ctrl(KeyCode::Char('c')), &dummy_retry());
        assert!(s.should_quit);
    }

    #[test]
    fn s_key_in_tail_switches_to_search() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('s')), &dummy_retry());
        assert_eq!(s.mode, Mode::Search);
    }

    #[test]
    fn esc_in_search_switches_to_tail() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Esc), &dummy_retry());
        assert_eq!(s.mode, Mode::Tail);
    }

    #[test]
    fn slash_in_tail_list_focuses_filter() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('/')), &dummy_retry());
        assert_eq!(s.focus, Focus::Filter);
    }

    #[test]
    fn q_in_tail_list_quits() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('q')), &dummy_retry());
        assert!(s.should_quit);
    }

    #[test]
    fn tab_in_search_mode_cycles_focus() {
        use super::super::state::{Mode, SearchFocus};
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Tab), &dummy_retry());
        assert_eq!(s.search_focus, SearchFocus::From);
        dispatch(&mut s, k(KeyCode::Tab), &dummy_retry());
        assert_eq!(s.search_focus, SearchFocus::To);
    }

    #[test]
    fn typing_in_search_query_appends_to_search_q() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        dispatch(&mut s, k(KeyCode::Char('a')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('b')), &dummy_retry());
        assert_eq!(s.search_q, "ab");
    }

    #[test]
    fn enter_in_query_marks_status_running() {
        use super::super::state::{Mode, SearchStatus};
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        s.search_q = "boom".into();
        dispatch(&mut s, k(KeyCode::Enter), &dummy_retry());
        assert!(matches!(s.search_status, SearchStatus::Running));
    }

    #[test]
    fn backspace_in_focused_field_pops_char() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        s.search_q = "abc".into();
        dispatch(&mut s, k(KeyCode::Backspace), &dummy_retry());
        assert_eq!(s.search_q, "ab");
    }

    #[test]
    fn enter_in_tail_list_opens_detail_for_selected_record() {
        use chrono::{TimeZone, Utc};
        use sift_core::{Level, Record};
        use std::collections::BTreeMap;
        let r = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: "hi".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        };
        let mut s = AppState::new();
        s.push_record(r);
        dispatch(&mut s, k(KeyCode::Enter), &dummy_retry());
        assert!(s.detail.is_some());
    }

    #[test]
    fn uppercase_w_in_tail_toggles_warn() {
        let mut s = AppState::new();
        assert!(s.level_set.warn);
        dispatch(&mut s, k(KeyCode::Char('W')), &dummy_retry());
        assert!(!s.level_set.warn);
    }

    #[test]
    fn space_in_tail_list_toggles_pause() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(' ')), &dummy_retry());
        assert!(s.paused);
        dispatch(&mut s, k(KeyCode::Char(' ')), &dummy_retry());
        assert!(!s.paused);
    }

    #[test]
    fn question_mark_in_tail_list_toggles_help() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('?')), &dummy_retry());
        assert!(s.help_open);
    }

    #[test]
    fn question_mark_in_filter_focus_does_not_open_help() {
        let mut s = AppState::new();
        s.focus_filter();
        dispatch(&mut s, k(KeyCode::Char('?')), &dummy_retry());
        assert!(!s.help_open);
        assert_eq!(s.filter.value(), "?");
    }

    #[test]
    fn esc_closes_detail_without_exiting_mode() {
        use super::super::state::Mode;
        use chrono::{TimeZone, Utc};
        use sift_core::{Level, Record};
        use std::collections::BTreeMap;
        let r = Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: "x".into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        };
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        s.open_detail(r);
        dispatch(&mut s, k(KeyCode::Esc), &dummy_retry());
        assert!(s.detail.is_none());
        assert_eq!(s.mode, Mode::Search);
    }

    #[test]
    fn r_key_in_tail_list_sends_retry_signal() {
        let (tx, mut rx) = mpsc::channel::<()>(1);
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char('r')), &tx);
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn colon_in_tail_list_opens_command_prompt() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        assert!(s.command_open);
        assert_eq!(s.command_input.value(), "");
    }

    #[test]
    fn typing_in_command_prompt_appends() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('p')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('a')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('u')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('s')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('e')), &dummy_retry());
        assert_eq!(s.command_input.value(), "pause");
    }

    #[test]
    fn escape_in_command_prompt_cancels() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('p')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Esc), &dummy_retry());
        assert!(!s.command_open);
        assert_eq!(s.command_input.value(), "");
    }

    #[test]
    fn enter_in_command_prompt_parses_and_stashes_pending_action() {
        use super::super::command::CommandAction;
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        for c in "pause".chars() {
            dispatch(&mut s, k(KeyCode::Char(c)), &dummy_retry());
        }
        dispatch(&mut s, k(KeyCode::Enter), &dummy_retry());
        assert!(!s.command_open);
        assert_eq!(s.pending_command, Some(CommandAction::Pause));
    }

    #[test]
    fn enter_on_bad_command_sets_flash_err() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        for c in "frobnicate".chars() {
            dispatch(&mut s, k(KeyCode::Char(c)), &dummy_retry());
        }
        dispatch(&mut s, k(KeyCode::Enter), &dummy_retry());
        assert!(s.pending_command.is_none());
        assert!(matches!(
            s.flash.as_ref().map(|f| f.kind),
            Some(super::super::state::FlashKind::Err)
        ));
    }

    #[test]
    fn tab_in_command_prompt_completes_prefix() {
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Char('p')), &dummy_retry());
        dispatch(&mut s, k(KeyCode::Tab), &dummy_retry());
        // Only verb starting with 'p' is "pause".
        assert_eq!(s.command_input.value(), "pause");
    }

    #[test]
    fn colon_in_filter_focus_does_not_open_command_prompt() {
        let mut s = AppState::new();
        s.focus_filter();
        dispatch(&mut s, k(KeyCode::Char(':')), &dummy_retry());
        assert!(!s.command_open);
        assert_eq!(s.filter.value(), ":");
    }
}
