//! Thin convenience layer over `tui-input::Input`. Gives us a single place to
//! set Insert-mode conventions (Ctrl+A home, Ctrl+E end, typed chars append,
//! Esc/Enter exit) and a single place to migrate if we ever swap backends.

use crossterm::event::{Event, KeyEvent};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

/// Feed a keystroke into an `Input`. Caller is responsible for filtering out
/// Esc/Enter (which have view-specific meanings).
pub fn feed(input: &mut Input, key: KeyEvent) {
    input.handle_event(&Event::Key(key));
}

/// Reset input to empty.
#[allow(dead_code)]
pub fn clear(input: &mut Input) {
    *input = Input::default();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    fn k(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn feed_appends_chars() {
        let mut i = Input::default();
        feed(&mut i, k('a'));
        feed(&mut i, k('b'));
        feed(&mut i, k('c'));
        assert_eq!(i.value(), "abc");
    }

    #[test]
    fn feed_backspace_deletes_last() {
        let mut i = Input::new("abc".into());
        feed(
            &mut i,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(i.value(), "ab");
    }

    #[test]
    fn clear_resets_to_empty() {
        let mut i = Input::new("hello".into());
        clear(&mut i);
        assert_eq!(i.value(), "");
    }
}
