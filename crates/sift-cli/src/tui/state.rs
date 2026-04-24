//! Pure state for the TUI. No I/O, no terminal, no ratatui. Fully testable.

use std::collections::VecDeque;

use sift_api::TailFrame;
use sift_core::{Level, Record};
use tui_input::Input;

/// Maximum records kept in memory. Older records are dropped.
pub const RECORD_CAP: usize = 500;

/// Events the TUI consumes from the WebSocket task.
#[derive(Debug, Clone)]
pub enum WsEvent {
    Status(Status),
    Frame(TailFrame),
    /// WebSocket terminated; the TUI shows "disconnected" and stops receiving.
    /// The current `ws::run` loop reconnects indefinitely and never emits this
    /// directly — the state layer still handles it so a future path (e.g. a
    /// terminal failure) can surface it without reshaping the enum.
    #[allow(dead_code)]
    Disconnected,
}

/// Connection status shown in the header pill.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    Reconnecting { attempt: u32, next_in_ms: u32 },
    Disconnected,
}

impl Status {
    #[allow(dead_code)]
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Connecting => "connecting".into(),
            Self::Connected => "connected".into(),
            Self::Reconnecting {
                attempt,
                next_in_ms,
            } => {
                format!(
                    "reconnecting (attempt {}, next in {}s)",
                    attempt,
                    next_in_ms / 1000
                )
            }
            Self::Disconnected => "disconnected".into(),
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

/// Which top-level view the TUI shows.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Mode {
    Tail,
    Search,
}

/// Which search input field currently receives typed characters.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SearchFocus {
    Query,
    From,
    To,
}

/// Outcome of the last search request.
#[derive(Debug, Clone, PartialEq)]
pub enum SearchStatus {
    Idle,
    Running,
    Ok { scanned: usize, elapsed_ms: u64 },
    Err(String),
}

// One bool per level is the natural representation here: each bit maps to a
// keybind (T/D/I/W/E/F) and to a character in the status indicator. A
// state-machine or bitflags crate would add ceremony without clarity.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct LevelSet {
    pub trace: bool,
    pub debug: bool,
    pub info: bool,
    pub warn: bool,
    pub error: bool,
    pub fatal: bool,
}

impl Default for LevelSet {
    fn default() -> Self {
        Self {
            trace: false,
            debug: false,
            info: true,
            warn: true,
            error: true,
            fatal: true,
        }
    }
}

impl LevelSet {
    #[must_use]
    pub fn allows(self, lvl: Level) -> bool {
        match lvl {
            Level::Trace => self.trace,
            Level::Debug => self.debug,
            Level::Info => self.info,
            Level::Warn => self.warn,
            Level::Error => self.error,
            Level::Fatal => self.fatal,
        }
    }
}

/// Whether the list is pinned to newest records or to a fixed offset.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ScrollMode {
    /// Always show the newest records at the bottom.
    Follow,
    /// Pinned: `offset` = index of the top visible record in the *filtered* view.
    Locked { offset: usize },
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TimestampMode {
    Absolute,
    Relative,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FlashKind {
    Ok,
    Err,
}

#[derive(Debug, Clone)]
pub struct Flash {
    pub msg: String,
    pub kind: FlashKind,
    pub show_until: std::time::Instant,
}

// Each bool represents a distinct, independently togglable UI state (quit,
// pause, help, command-prompt). A state-machine enum would obscure the fact
// that these flags are orthogonal and can be independently true/false.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct AppState {
    records: VecDeque<Record>,
    pub status: Status,
    pub focus: Focus,
    pub filter: Input,
    pub scroll: ScrollMode,
    pub should_quit: bool,
    pub mode: Mode,
    pub search_q: tui_input::Input,
    pub search_from: tui_input::Input,
    pub search_to: tui_input::Input,
    pub search_focus: SearchFocus,
    pub search_results: Vec<Record>,
    pub search_status: SearchStatus,
    pub search_cursor: usize,
    pub detail: Option<Record>,
    /// Some(idx) selects a specific tail row; None means "last".
    pub tail_cursor: Option<usize>,
    pub level_set: LevelSet,
    pub paused: bool,
    pending: VecDeque<Record>,
    pub help_open: bool,

    #[allow(dead_code)]
    pub theme: crate::tui::theme::Theme,
    pub focus_pane: crate::tui::focus::Pane,
    pub timestamp_mode: TimestampMode,

    #[allow(dead_code)]
    pub rate_history: VecDeque<u32>,
    #[allow(dead_code)]
    pub rate_current: u32,
    #[allow(dead_code)]
    pub(super) rate_second_start: std::time::Instant,

    pub flash: Option<Flash>,

    pub command_open: bool,
    pub command_input: Input,
    pub pending_command: Option<crate::tui::command::CommandAction>,
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
            filter: Input::default(),
            scroll: ScrollMode::Follow,
            should_quit: false,
            mode: Mode::Tail,
            search_q: tui_input::Input::default(),
            search_from: tui_input::Input::default(),
            search_to: tui_input::Input::default(),
            search_focus: SearchFocus::Query,
            search_results: Vec::new(),
            search_status: SearchStatus::Idle,
            search_cursor: 0,
            detail: None,
            tail_cursor: None,
            level_set: LevelSet::default(),
            paused: false,
            pending: VecDeque::with_capacity(RECORD_CAP),
            help_open: false,

            theme: crate::tui::theme::Theme::dark(),
            focus_pane: crate::tui::focus::Pane::Records,
            timestamp_mode: TimestampMode::Absolute,

            rate_history: VecDeque::with_capacity(60),
            rate_current: 0,
            rate_second_start: std::time::Instant::now(),

            flash: None,

            command_open: false,
            command_input: Input::default(),
            pending_command: None,
        }
    }

    pub fn set_mode(&mut self, m: Mode) {
        self.mode = m;
        // Switching modes always returns focus to the list to avoid stale
        // filter focus leaking into search.
        // TODO(T17): also set self.focus_pane = Pane::Records once dual focus is consolidated.
        self.focus = Focus::List;
    }

    /// Number of records held in memory (unfiltered).
    /// Used only in tests; production code uses `filtered()` directly.
    #[allow(dead_code)]
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn push_record(&mut self, record: Record) {
        if self.paused {
            if self.pending.len() == RECORD_CAP {
                self.pending.pop_front();
            }
            self.pending.push_back(record);
        } else {
            if self.records.len() == RECORD_CAP {
                self.records.pop_front();
            }
            self.records.push_back(record);
        }
    }

    pub fn toggle_pause(&mut self) {
        if self.paused {
            self.resume();
        } else {
            self.paused = true;
        }
    }

    pub fn resume(&mut self) {
        self.paused = false;
        while let Some(r) = self.pending.pop_front() {
            if self.records.len() == RECORD_CAP {
                self.records.pop_front();
            }
            self.records.push_back(r);
        }
    }

    #[allow(dead_code)]
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn apply_ws_event(&mut self, evt: WsEvent) {
        match evt {
            WsEvent::Status(s) => self.status = s,
            WsEvent::Frame(TailFrame::Record(r)) => {
                self.record_rate_inc();
                self.push_record(r);
            }
            WsEvent::Frame(_) => {}
            WsEvent::Disconnected => self.status = Status::Disconnected,
        }
    }

    /// Returns the filtered record slice as owned references.
    /// Ordering matches insertion order (oldest first).
    #[must_use]
    pub fn filtered(&self) -> Vec<&Record> {
        let needle = self.filter.value();
        self.records
            .iter()
            .filter(|r| self.level_set.allows(r.level))
            .filter(|r| needle.is_empty() || r.body.contains(needle))
            .collect()
    }

    #[must_use]
    pub fn filtered_search(&self) -> Vec<&Record> {
        self.search_results
            .iter()
            .filter(|r| self.level_set.allows(r.level))
            .collect()
    }

    #[allow(dead_code)]
    pub fn records_iter(&self) -> impl Iterator<Item = &Record> {
        self.records.iter()
    }

    pub fn toggle_level(&mut self, lvl: Level) {
        let s = &mut self.level_set;
        match lvl {
            Level::Trace => s.trace = !s.trace,
            Level::Debug => s.debug = !s.debug,
            Level::Info => s.info = !s.info,
            Level::Warn => s.warn = !s.warn,
            Level::Error => s.error = !s.error,
            Level::Fatal => s.fatal = !s.fatal,
        }
    }

    #[allow(dead_code)]
    pub fn set_filter(&mut self, s: String) {
        self.filter = Input::new(s);
        // Editing the filter resets to follow-mode so the newest hits stay
        // visible. Without this, scroll offsets become stale silently.
        self.scroll = ScrollMode::Follow;
    }

    pub fn focus_filter(&mut self) {
        // TODO(T17): also set self.focus_pane = Pane::Filter once dual focus is consolidated.
        self.focus = Focus::Filter;
    }

    pub fn unfocus_filter(&mut self) {
        // TODO(T17): also set self.focus_pane = Pane::Records once dual focus is consolidated.
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

    // Explicit six-arm match keeps the (current, direction) → next mapping
    // readable at a glance. Clippy would otherwise ask us to merge arms with
    // identical bodies, which obscures the state-machine shape.
    #[allow(clippy::match_same_arms)]
    pub fn cycle_search_focus(&mut self, forward: bool) {
        use SearchFocus::{From, Query, To};
        self.search_focus = match (self.search_focus, forward) {
            (Query, true) => From,
            (From, true) => To,
            (To, true) => Query,
            (Query, false) => To,
            (From, false) => Query,
            (To, false) => From,
        };
    }

    pub fn search_cursor_down(&mut self) {
        if self.search_cursor + 1 < self.search_results.len() {
            self.search_cursor += 1;
        }
    }

    pub fn search_cursor_up(&mut self) {
        self.search_cursor = self.search_cursor.saturating_sub(1);
    }

    /// Convenience for the HTTP task — replaces results and resets the cursor.
    pub fn set_search_results(&mut self, records: Vec<Record>, scanned: usize, elapsed_ms: u64) {
        self.search_results = records;
        self.search_cursor = 0;
        self.search_status = SearchStatus::Ok {
            scanned,
            elapsed_ms,
        };
    }

    pub fn open_detail(&mut self, r: Record) {
        self.detail = Some(r);
    }

    pub fn close_detail(&mut self) {
        self.detail = None;
    }

    pub fn toggle_help(&mut self) {
        self.help_open = !self.help_open;
    }

    #[must_use]
    pub fn tail_cursor_or_last(&self) -> usize {
        let total = self.filtered().len();
        match self.tail_cursor {
            Some(c) => c.min(total.saturating_sub(1)),
            None => total.saturating_sub(1),
        }
    }

    pub fn tail_cursor_up(&mut self) {
        let cur = self.tail_cursor_or_last();
        self.tail_cursor = Some(cur.saturating_sub(1));
    }

    pub fn tail_cursor_down(&mut self) {
        let total = self.filtered().len();
        let cur = self.tail_cursor_or_last();
        self.tail_cursor = Some((cur + 1).min(total.saturating_sub(1)));
    }

    /// Reset cursor to "follow the tail" — used when new records arrive and
    /// the user hasn't manually moved the cursor.
    pub fn tail_cursor_follow(&mut self) {
        self.tail_cursor = None;
    }

    pub fn toggle_timestamp_mode(&mut self) {
        self.timestamp_mode = match self.timestamp_mode {
            TimestampMode::Absolute => TimestampMode::Relative,
            TimestampMode::Relative => TimestampMode::Absolute,
        };
    }

    pub fn record_rate_inc(&mut self) {
        self.rate_current = self.rate_current.saturating_add(1);
    }

    /// Advance the rate ring by one second. Pushes `rate_current` and resets it.
    /// Caller (the 1 Hz tick in `mod.rs`) invokes this when `now() - rate_second_start >= 1s`.
    pub fn rate_tick(&mut self) {
        if self.rate_history.len() == 60 {
            self.rate_history.pop_front();
        }
        self.rate_history.push_back(self.rate_current);
        self.rate_current = 0;
        self.rate_second_start = std::time::Instant::now();
    }

    /// Average records-per-second over the last minute, as an integer.
    #[allow(dead_code)]
    #[must_use]
    pub fn rate_per_sec(&self) -> u32 {
        if self.rate_history.is_empty() {
            return 0;
        }
        let sum: u32 = self.rate_history.iter().sum();
        sum / u32::try_from(self.rate_history.len()).unwrap_or(1)
    }

    pub fn expire_flash_if_due(&mut self) {
        if let Some(f) = &self.flash {
            if std::time::Instant::now() >= f.show_until {
                self.flash = None;
            }
        }
    }

    #[allow(dead_code)]
    pub fn set_flash_ok(&mut self, msg: impl Into<String>) {
        self.flash = Some(Flash {
            msg: msg.into(),
            kind: FlashKind::Ok,
            show_until: std::time::Instant::now() + std::time::Duration::from_secs(2),
        });
    }

    pub fn set_flash_err(&mut self, msg: impl Into<String>) {
        self.flash = Some(Flash {
            msg: msg.into(),
            kind: FlashKind::Err,
            show_until: std::time::Instant::now() + std::time::Duration::from_secs(2),
        });
    }

    pub fn open_command_prompt(&mut self) {
        self.command_open = true;
        self.command_input = Input::default();
        self.focus_pane = crate::tui::focus::Pane::CommandPrompt;
    }

    pub fn close_command_prompt(&mut self) {
        self.command_open = false;
        self.command_input = Input::default();
        // Return focus to the default pane for the current mode.
        self.focus_pane = crate::tui::focus::default_pane(self.mode);
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
    fn new_state_defaults_to_tail_mode() {
        let s = AppState::new();
        assert_eq!(s.mode, Mode::Tail);
    }

    #[test]
    fn set_mode_switches_tail_and_search() {
        let mut s = AppState::new();
        s.set_mode(Mode::Search);
        assert_eq!(s.mode, Mode::Search);
        s.set_mode(Mode::Tail);
        assert_eq!(s.mode, Mode::Tail);
    }

    #[test]
    fn new_state_is_follow_connecting_list_empty() {
        let s = AppState::new();
        assert_eq!(s.status, Status::Connecting);
        assert_eq!(s.focus, Focus::List);
        assert_eq!(s.scroll, ScrollMode::Follow);
        assert_eq!(s.record_count(), 0);
        assert!(s.filter.value().is_empty());
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

    #[test]
    fn new_state_has_empty_search_and_query_focus() {
        let s = AppState::new();
        assert_eq!(s.search_q.value(), "");
        assert_eq!(s.search_from.value(), "");
        assert_eq!(s.search_to.value(), "");
        assert_eq!(s.search_focus, SearchFocus::Query);
        assert!(matches!(s.search_status, SearchStatus::Idle));
        assert!(s.search_results.is_empty());
        assert_eq!(s.search_cursor, 0);
    }

    #[test]
    fn cycle_search_focus_moves_forward_and_wraps() {
        let mut s = AppState::new();
        s.cycle_search_focus(true);
        assert_eq!(s.search_focus, SearchFocus::From);
        s.cycle_search_focus(true);
        assert_eq!(s.search_focus, SearchFocus::To);
        s.cycle_search_focus(true);
        assert_eq!(s.search_focus, SearchFocus::Query);
    }

    #[test]
    fn cycle_search_focus_backward_wraps() {
        let mut s = AppState::new();
        s.cycle_search_focus(false);
        assert_eq!(s.search_focus, SearchFocus::To);
    }

    #[test]
    fn search_cursor_up_down_clamp() {
        let mut s = AppState::new();
        s.search_results = vec![rec("a"), rec("b"), rec("c")];
        s.search_cursor_down();
        assert_eq!(s.search_cursor, 1);
        s.search_cursor_down();
        s.search_cursor_down();
        assert_eq!(s.search_cursor, 2); // clamped
        s.search_cursor_up();
        assert_eq!(s.search_cursor, 1);
        s.search_cursor = 0;
        s.search_cursor_up();
        assert_eq!(s.search_cursor, 0); // clamped
    }

    #[test]
    fn open_detail_clones_record_into_detail_slot() {
        let mut s = AppState::new();
        let r = rec("hello");
        s.open_detail(r.clone());
        assert!(s.detail.is_some());
        assert_eq!(s.detail.as_ref().unwrap().body, "hello");
    }

    #[test]
    fn close_detail_clears_slot() {
        let mut s = AppState::new();
        s.open_detail(rec("x"));
        s.close_detail();
        assert!(s.detail.is_none());
    }

    #[test]
    fn tail_cursor_defaults_to_last_and_moves() {
        let mut s = AppState::new();
        s.push_record(rec("a"));
        s.push_record(rec("b"));
        s.push_record(rec("c"));
        assert_eq!(s.tail_cursor_or_last(), 2);
        s.tail_cursor_up();
        assert_eq!(s.tail_cursor_or_last(), 1);
        s.tail_cursor_down();
        assert_eq!(s.tail_cursor_or_last(), 2);
    }

    #[test]
    fn level_set_defaults_to_info_warn_error_fatal() {
        let s = AppState::new();
        assert!(s.level_set.info);
        assert!(s.level_set.warn);
        assert!(s.level_set.error);
        assert!(s.level_set.fatal);
        assert!(!s.level_set.debug);
        assert!(!s.level_set.trace);
    }

    #[test]
    fn toggle_level_flips_bit() {
        let mut s = AppState::new();
        s.toggle_level(sift_core::Level::Warn);
        assert!(!s.level_set.warn);
        s.toggle_level(sift_core::Level::Warn);
        assert!(s.level_set.warn);
    }

    #[test]
    fn filtered_hides_records_whose_level_is_off() {
        let mut s = AppState::new();
        let mut r = rec("x");
        r.level = sift_core::Level::Error;
        s.push_record(r.clone());
        let mut r2 = rec("y");
        r2.level = sift_core::Level::Debug;
        s.push_record(r2);
        // Default: info+warn+error+fatal on, debug off.
        assert_eq!(s.filtered().len(), 1);
        assert_eq!(s.filtered()[0].body, "x");
    }

    #[test]
    fn pausing_routes_new_records_to_pending_buffer() {
        let mut s = AppState::new();
        s.paused = true;
        s.apply_ws_event(WsEvent::Frame(TailFrame::Record(rec("a"))));
        assert_eq!(s.record_count(), 0);
        assert_eq!(s.pending_count(), 1);
    }

    #[test]
    fn resume_merges_pending_into_main_and_clears_pending() {
        let mut s = AppState::new();
        s.paused = true;
        s.apply_ws_event(WsEvent::Frame(TailFrame::Record(rec("a"))));
        s.apply_ws_event(WsEvent::Frame(TailFrame::Record(rec("b"))));
        s.resume();
        assert_eq!(s.record_count(), 2);
        assert_eq!(s.pending_count(), 0);
        assert!(!s.paused);
    }

    #[test]
    fn pending_buffer_caps_at_record_cap() {
        let mut s = AppState::new();
        s.paused = true;
        for i in 0..(RECORD_CAP + 7) {
            s.apply_ws_event(WsEvent::Frame(TailFrame::Record(rec(&format!("m{i}")))));
        }
        assert_eq!(s.pending_count(), RECORD_CAP);
    }

    #[test]
    fn help_open_defaults_false_and_toggles() {
        let mut s = AppState::new();
        assert!(!s.help_open);
        s.toggle_help();
        assert!(s.help_open);
        s.toggle_help();
        assert!(!s.help_open);
    }

    #[test]
    fn new_state_has_theme_focus_timestamp_defaults() {
        use super::super::focus::Pane;
        use super::super::theme::Theme;
        let s = AppState::new();
        // Theme::default() == dark().
        assert_eq!(s.theme.accent, Theme::dark().accent);
        // Default focus is Records (tail mode default).
        assert_eq!(s.focus_pane, Pane::Records);
        // Default timestamps are absolute.
        assert_eq!(s.timestamp_mode, TimestampMode::Absolute);
    }

    #[test]
    fn toggle_timestamp_mode_flips() {
        let mut s = AppState::new();
        s.toggle_timestamp_mode();
        assert_eq!(s.timestamp_mode, TimestampMode::Relative);
        s.toggle_timestamp_mode();
        assert_eq!(s.timestamp_mode, TimestampMode::Absolute);
    }

    #[test]
    fn record_rate_tick_rotates_history() {
        let mut s = AppState::new();
        s.record_rate_inc();
        s.record_rate_inc();
        s.record_rate_inc();
        // Force the tick boundary (simulate a second elapsing):
        s.rate_tick();
        assert_eq!(s.rate_history.back().copied(), Some(3));
        assert_eq!(s.rate_current, 0);

        // After 65 ticks the deque should be capped at 60.
        for _ in 0..65 {
            s.rate_tick();
        }
        assert_eq!(s.rate_history.len(), 60);
    }

    #[test]
    fn rate_per_sec_is_60_window_average_as_integer() {
        let mut s = AppState::new();
        // Put 120 records across 60 seconds = 2/s avg.
        for _ in 0..60 {
            s.record_rate_inc();
            s.record_rate_inc();
            s.rate_tick();
        }
        assert_eq!(s.rate_per_sec(), 2);
    }

    #[test]
    fn flash_expires_after_show_until() {
        use std::time::{Duration, Instant};
        let mut s = AppState::new();
        let past = Instant::now().checked_sub(Duration::from_secs(5)).unwrap();
        s.flash = Some(Flash {
            msg: "old".into(),
            kind: FlashKind::Ok,
            show_until: past,
        });
        s.expire_flash_if_due();
        assert!(s.flash.is_none());
    }
}
