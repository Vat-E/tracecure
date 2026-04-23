//! Pure state for the TUI. No I/O, no terminal, no ratatui. Fully testable.

use std::collections::VecDeque;

use sift_api::TailFrame;
use sift_core::Record;

/// Maximum records kept in memory. Older records are dropped.
pub const RECORD_CAP: usize = 500;

/// Events the TUI consumes from the WebSocket task.
#[derive(Debug, Clone)]
pub enum WsEvent {
    Status(Status),
    Frame(TailFrame),
    /// WebSocket terminated; the TUI shows "disconnected" and stops receiving.
    Disconnected,
}

/// Connection status shown in the header pill.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    Disconnected,
}

impl Status {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
        }
    }
}

/// Where keyboard input goes.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Focus {
    /// Keys affect the record list (scroll, quit).
    List,
    /// Keys edit the filter string.
    Filter,
}

/// Whether the list is pinned to newest records or to a fixed offset.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScrollMode {
    /// Always show the newest records at the bottom.
    Follow,
    /// Pinned: `offset` = index of the top visible record in the *filtered* view.
    Locked { offset: usize },
}

#[derive(Debug)]
pub struct AppState {
    records: VecDeque<Record>,
    pub status: Status,
    pub focus: Focus,
    pub filter: String,
    pub scroll: ScrollMode,
    pub should_quit: bool,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: VecDeque::with_capacity(RECORD_CAP),
            status: Status::Connecting,
            focus: Focus::List,
            filter: String::new(),
            scroll: ScrollMode::Follow,
            should_quit: false,
        }
    }

    /// Number of records held in memory (unfiltered).
    /// Used only in tests; production code uses `filtered()` directly.
    #[allow(dead_code)]
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn push_record(&mut self, record: Record) {
        if self.records.len() == RECORD_CAP {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    pub fn apply_ws_event(&mut self, evt: WsEvent) {
        match evt {
            WsEvent::Status(s) => self.status = s,
            WsEvent::Frame(TailFrame::Record(r)) => self.push_record(r),
            WsEvent::Frame(_) => {}
            WsEvent::Disconnected => self.status = Status::Disconnected,
        }
    }

    /// Returns the filtered record slice as owned references.
    /// Ordering matches insertion order (oldest first).
    #[must_use]
    pub fn filtered(&self) -> Vec<&Record> {
        if self.filter.is_empty() {
            return self.records.iter().collect();
        }
        let needle = &self.filter;
        self.records
            .iter()
            .filter(|r| r.body.contains(needle))
            .collect()
    }

    pub fn set_filter(&mut self, s: String) {
        self.filter = s;
        // Editing the filter resets to follow-mode so the newest hits stay
        // visible. Without this, scroll offsets become stale silently.
        self.scroll = ScrollMode::Follow;
    }

    pub fn focus_filter(&mut self) {
        self.focus = Focus::Filter;
    }

    pub fn unfocus_filter(&mut self) {
        self.focus = Focus::List;
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    /// Scroll up by `n` rows. Enters `Locked` mode if currently following.
    pub fn scroll_up(&mut self, n: usize) {
        let filtered_len = self.filtered().len();
        let current = match self.scroll {
            ScrollMode::Follow => filtered_len.saturating_sub(1),
            ScrollMode::Locked { offset } => offset,
        };
        self.scroll = ScrollMode::Locked {
            offset: current.saturating_sub(n),
        };
    }

    /// Scroll down by `n` rows. Re-enters `Follow` when we reach the tail.
    pub fn scroll_down(&mut self, n: usize) {
        let filtered_len = self.filtered().len();
        let current = match self.scroll {
            ScrollMode::Follow => return, // already at tail
            ScrollMode::Locked { offset } => offset,
        };
        let new_offset = current.saturating_add(n);
        if new_offset + 1 >= filtered_len {
            self.scroll = ScrollMode::Follow;
        } else {
            self.scroll = ScrollMode::Locked { offset: new_offset };
        }
    }

    /// Return to follow mode (e.g. End key).
    pub fn follow(&mut self) {
        self.scroll = ScrollMode::Follow;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use sift_core::{Level, Record};
    use std::collections::BTreeMap;

    fn rec(body: &str) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: Level::Info,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn new_state_is_follow_connecting_list_empty() {
        let s = AppState::new();
        assert_eq!(s.status, Status::Connecting);
        assert_eq!(s.focus, Focus::List);
        assert_eq!(s.scroll, ScrollMode::Follow);
        assert_eq!(s.record_count(), 0);
        assert!(s.filter.is_empty());
        assert!(!s.should_quit);
    }

    #[test]
    fn push_record_caps_at_record_cap() {
        let mut s = AppState::new();
        for i in 0..(RECORD_CAP + 7) {
            s.push_record(rec(&format!("msg {i}")));
        }
        assert_eq!(s.record_count(), RECORD_CAP);
        // Oldest records were dropped.
        assert_eq!(s.filtered()[0].body, format!("msg {}", 7));
    }

    #[test]
    fn filter_empty_returns_all() {
        let mut s = AppState::new();
        s.push_record(rec("alpha"));
        s.push_record(rec("beta"));
        assert_eq!(s.filtered().len(), 2);
    }

    #[test]
    fn filter_substring_filters_body() {
        let mut s = AppState::new();
        s.push_record(rec("hello world"));
        s.push_record(rec("goodbye"));
        s.set_filter("hello".into());
        assert_eq!(s.filtered().len(), 1);
        assert_eq!(s.filtered()[0].body, "hello world");
    }

    #[test]
    fn setting_filter_returns_to_follow_mode() {
        let mut s = AppState::new();
        s.push_record(rec("one"));
        s.push_record(rec("two"));
        s.scroll_up(1);
        assert!(matches!(s.scroll, ScrollMode::Locked { .. }));
        s.set_filter("on".into());
        assert_eq!(s.scroll, ScrollMode::Follow);
    }

    #[test]
    fn scroll_down_past_tail_returns_to_follow() {
        let mut s = AppState::new();
        for i in 0..5 {
            s.push_record(rec(&format!("m{i}")));
        }
        s.scroll_up(3);
        s.scroll_down(10);
        assert_eq!(s.scroll, ScrollMode::Follow);
    }

    #[test]
    fn apply_ws_event_status_updates_status() {
        let mut s = AppState::new();
        s.apply_ws_event(WsEvent::Status(Status::Connected));
        assert_eq!(s.status, Status::Connected);
    }

    #[test]
    fn apply_ws_event_record_frame_pushes_record() {
        let mut s = AppState::new();
        s.apply_ws_event(WsEvent::Frame(TailFrame::Record(rec("x"))));
        assert_eq!(s.record_count(), 1);
    }

    #[test]
    fn apply_ws_event_heartbeat_is_ignored() {
        let mut s = AppState::new();
        s.apply_ws_event(WsEvent::Frame(TailFrame::Heartbeat));
        assert_eq!(s.record_count(), 0);
    }

    #[test]
    fn apply_ws_event_unknown_is_ignored() {
        let mut s = AppState::new();
        s.apply_ws_event(WsEvent::Frame(TailFrame::Unknown));
        assert_eq!(s.record_count(), 0);
    }

    #[test]
    fn apply_ws_event_error_frame_is_ignored_at_state_layer() {
        // ws.rs is responsible for logging TailFrame::Error and (eventually)
        // translating it into a status change. At the state layer it is a
        // no-op: no record pushed, status unchanged.
        let mut s = AppState::new();
        s.status = Status::Connected;
        s.apply_ws_event(WsEvent::Frame(TailFrame::Error {
            message: "lagged".into(),
        }));
        assert_eq!(s.record_count(), 0);
        assert_eq!(s.status, Status::Connected);
    }

    #[test]
    fn apply_ws_event_disconnected_sets_status_to_disconnected() {
        let mut s = AppState::new();
        s.apply_ws_event(WsEvent::Disconnected);
        assert_eq!(s.status, Status::Disconnected);
    }

    #[test]
    fn focus_filter_and_unfocus_filter_toggle_focus() {
        let mut s = AppState::new();
        assert_eq!(s.focus, Focus::List);
        s.focus_filter();
        assert_eq!(s.focus, Focus::Filter);
        s.unfocus_filter();
        assert_eq!(s.focus, Focus::List);
    }
}
