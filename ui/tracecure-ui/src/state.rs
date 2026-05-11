//! `AppState` — plain Rust, no Leptos. The shell component wraps an
//! `AppState` in an `RwSignal` and every component that reads/writes
//! state goes through that.
//!
//! Methods mirror the TUI's `AppState` (crates/tracecure-cli/src/tui/state.rs)
//! wherever semantics match, so the mental model carries over.

use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracecure_core::{Level, Record};

use crate::command::CommandAction;

pub const RECORD_CAP: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampMode {
    Absolute,
    Relative,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeWindow {
    Preset(Duration),
    Custom {
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
}

impl TimeWindow {
    /// Default = Last 1 hour rolling.
    #[must_use]
    pub fn default_1h() -> Self {
        Self::Preset(Duration::from_secs(3600))
    }

    /// True when the window rolls forward with now (preset or custom-to-now).
    #[must_use]
    pub fn is_rolling(&self) -> bool {
        matches!(self, Self::Preset(_))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct LevelSet {
    pub info: bool,
    pub warn: bool,
    pub error: bool,
    pub fatal: bool,
    pub debug: bool,
    pub trace: bool,
}

impl LevelSet {
    /// Defaults: I/W/E/F on, D/T off (mirror TUI).
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            info: true,
            warn: true,
            error: true,
            fatal: true,
            debug: false,
            trace: false,
        }
    }

    #[must_use]
    pub fn allows(&self, lvl: Level) -> bool {
        match lvl {
            Level::Info => self.info,
            Level::Warn => self.warn,
            Level::Error => self.error,
            Level::Fatal => self.fatal,
            Level::Debug => self.debug,
            Level::Trace => self.trace,
        }
    }

    pub fn toggle(&mut self, lvl: Level) {
        match lvl {
            Level::Info => self.info = !self.info,
            Level::Warn => self.warn = !self.warn,
            Level::Error => self.error = !self.error,
            Level::Fatal => self.fatal = !self.fatal,
            Level::Debug => self.debug = !self.debug,
            Level::Trace => self.trace = !self.trace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashKind {
    Ok,
    Err,
}

#[derive(Debug, Clone)]
pub struct Flash {
    pub msg: String,
    pub kind: FlashKind,
    pub expires_at_ms: u64, // epoch millis; client clears when now > this
}

#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct AppState {
    // Data
    pub records: VecDeque<Record>,
    pub pending: Vec<Record>,
    pub pending_count: usize,
    pub status: Status,

    // View
    pub time_window: TimeWindow,
    pub level_set: LevelSet,
    pub timestamp_mode: TimestampMode,
    pub query: String,
    pub selected_row_index: Option<usize>,
    pub detail_expanded: bool,

    // Live / paused
    pub live: bool,
    pub at_bottom: bool,
    pub flash: Option<Flash>,

    // Command palette
    pub command_open: bool,
    pub command_input: String,
    pub pending_command: Option<CommandAction>,
    pub help_open: bool,
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
            pending: Vec::new(),
            pending_count: 0,
            status: Status::Disconnected,
            time_window: TimeWindow::default_1h(),
            level_set: LevelSet::defaults(),
            timestamp_mode: TimestampMode::Absolute,
            query: String::new(),
            selected_row_index: None,
            detail_expanded: false,
            live: true,
            at_bottom: true,
            flash: None,
            command_open: false,
            command_input: String::new(),
            pending_command: None,
            help_open: false,
        }
    }

    /// Push a record. If paused, it goes to `pending`. Caller is the
    /// WebSocket callback.
    pub fn push_record(&mut self, r: Record) {
        if self.live && self.at_bottom {
            if self.records.len() >= RECORD_CAP {
                self.records.pop_front();
            }
            self.records.push_back(r);
        } else {
            self.pending.push(r);
            self.pending_count = self.pending.len();
        }
    }

    /// Drain pending → records and clear counter. Called on resume.
    pub fn drain_pending(&mut self) {
        for r in self.pending.drain(..) {
            if self.records.len() >= RECORD_CAP {
                self.records.pop_front();
            }
            self.records.push_back(r);
        }
        self.pending_count = 0;
    }

    /// Replace records wholesale (e.g. after a windowed search refetch).
    pub fn replace_records(&mut self, rs: Vec<Record>) {
        self.records.clear();
        self.records.extend(rs);
        self.pending.clear();
        self.pending_count = 0;
    }

    /// User explicitly resumed: drain + scroll + live on.
    pub fn resume_live(&mut self) {
        self.drain_pending();
        self.live = true;
        self.at_bottom = true;
    }

    /// Scroll-up detection: set live=false when we leave the bottom.
    pub fn on_scroll_not_at_bottom(&mut self) {
        self.live = false;
        self.at_bottom = false;
    }

    /// Filtered view based on `level_set` + query substring.
    #[must_use]
    pub fn filtered(&self) -> Vec<&Record> {
        let q = self.query.as_str();
        self.records
            .iter()
            .filter(|r| self.level_set.allows(r.level))
            .filter(|r| q.is_empty() || r.body.contains(q))
            .collect()
    }

    pub fn toggle_level(&mut self, lvl: Level) {
        self.level_set.toggle(lvl);
    }

    pub fn toggle_timestamp_mode(&mut self) {
        self.timestamp_mode = match self.timestamp_mode {
            TimestampMode::Absolute => TimestampMode::Relative,
            TimestampMode::Relative => TimestampMode::Absolute,
        };
    }

    pub fn open_detail(&mut self, idx: usize) {
        self.selected_row_index = Some(idx);
        self.detail_expanded = true;
    }

    pub fn close_detail(&mut self) {
        self.detail_expanded = false;
    }

    pub fn clear_records(&mut self) {
        let cleared = self.records.len() + self.pending.len();
        self.records.clear();
        self.pending.clear();
        self.pending_count = 0;
        self.set_flash_ok(format!("cleared {cleared} records"));
    }

    pub fn set_flash_ok(&mut self, msg: impl Into<String>) {
        self.flash = Some(Flash {
            msg: msg.into(),
            kind: FlashKind::Ok,
            expires_at_ms: now_ms() + 2000,
        });
    }

    pub fn set_flash_err(&mut self, msg: impl Into<String>) {
        self.flash = Some(Flash {
            msg: msg.into(),
            kind: FlashKind::Err,
            expires_at_ms: now_ms() + 2000,
        });
    }

    pub fn expire_flash_if_due(&mut self) {
        if let Some(f) = &self.flash {
            if now_ms() >= f.expires_at_ms {
                self.flash = None;
            }
        }
    }

    pub fn open_command_prompt(&mut self) {
        self.command_open = true;
        self.command_input.clear();
    }

    pub fn close_command_prompt(&mut self) {
        self.command_open = false;
        self.command_input.clear();
    }

    pub fn toggle_help(&mut self) {
        self.help_open = !self.help_open;
    }
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ms = js_sys::Date::now() as u64;
    ms
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    use std::time::SystemTime;
    #[allow(clippy::cast_possible_truncation)]
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tracecure_core::{Level, Record};
    use std::collections::BTreeMap;

    fn rec(body: &str, lvl: Level) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap(),
            host: "h".into(),
            app: "a".into(),
            stream_id: "s".into(),
            level: lvl,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn push_when_live_and_at_bottom_goes_to_records() {
        let mut s = AppState::new();
        s.push_record(rec("a", Level::Info));
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.pending.len(), 0);
    }

    #[test]
    fn push_when_not_live_goes_to_pending() {
        let mut s = AppState::new();
        s.live = false;
        s.push_record(rec("a", Level::Info));
        assert_eq!(s.records.len(), 0);
        assert_eq!(s.pending.len(), 1);
        assert_eq!(s.pending_count, 1);
    }

    #[test]
    fn resume_live_drains_pending() {
        let mut s = AppState::new();
        s.live = false;
        s.at_bottom = false;
        s.push_record(rec("a", Level::Info));
        s.push_record(rec("b", Level::Info));
        s.resume_live();
        assert_eq!(s.records.len(), 2);
        assert_eq!(s.pending.len(), 0);
        assert_eq!(s.pending_count, 0);
        assert!(s.live);
        assert!(s.at_bottom);
    }

    #[test]
    fn record_cap_evicts_oldest() {
        let mut s = AppState::new();
        // Fill over capacity
        for _ in 0..(RECORD_CAP + 10) {
            s.push_record(rec("x", Level::Info));
        }
        assert_eq!(s.records.len(), RECORD_CAP);
    }

    #[test]
    fn filtered_respects_level_set() {
        let mut s = AppState::new();
        s.push_record(rec("info msg", Level::Info));
        s.push_record(rec("debug msg", Level::Debug));
        // default: D off
        assert_eq!(s.filtered().len(), 1);
        assert_eq!(s.filtered()[0].body, "info msg");
    }

    #[test]
    fn filtered_respects_query_substring() {
        let mut s = AppState::new();
        s.push_record(rec("hello world", Level::Info));
        s.push_record(rec("goodbye", Level::Info));
        s.query = "hello".into();
        let f = s.filtered();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].body, "hello world");
    }

    #[test]
    fn on_scroll_not_at_bottom_pauses_live() {
        let mut s = AppState::new();
        s.on_scroll_not_at_bottom();
        assert!(!s.live);
        assert!(!s.at_bottom);
    }

    #[test]
    fn toggle_level_works() {
        let mut s = AppState::new();
        assert!(s.level_set.info);
        s.toggle_level(Level::Info);
        assert!(!s.level_set.info);
    }

    #[test]
    fn toggle_timestamp_mode_flips() {
        let mut s = AppState::new();
        assert_eq!(s.timestamp_mode, TimestampMode::Absolute);
        s.toggle_timestamp_mode();
        assert_eq!(s.timestamp_mode, TimestampMode::Relative);
    }

    #[test]
    fn clear_records_counts_pending_in_flash() {
        let mut s = AppState::new();
        s.push_record(rec("a", Level::Info));
        s.live = false;
        s.push_record(rec("b", Level::Info));
        s.clear_records();
        assert_eq!(s.records.len(), 0);
        assert_eq!(s.pending.len(), 0);
        let msg = s.flash.as_ref().unwrap().msg.clone();
        assert!(msg.contains('2'), "expected flash to mention 2, got: {msg}");
    }

    #[test]
    fn open_detail_sets_index_and_expanded() {
        let mut s = AppState::new();
        s.push_record(rec("a", Level::Info));
        s.open_detail(0);
        assert_eq!(s.selected_row_index, Some(0));
        assert!(s.detail_expanded);
        s.close_detail();
        assert!(!s.detail_expanded);
    }
}
