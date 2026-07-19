//! `AppState` — plain Rust, no Leptos. The shell component wraps an
//! `AppState` in an `RwSignal` and every component that reads/writes
//! state goes through that.
//!
//! Methods mirror the TUI's `AppState` (crates/tracecure-cli/src/tui/state.rs)
//! wherever semantics match, so the mental model carries over.

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracecure_core::{Level, Record};

use crate::command::CommandAction;

pub const RECORD_CAP: usize = 5000;
pub const RATE_BUCKET_COUNT: usize = 30;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateBucket {
    pub total: u32,
    pub warn_count: u32,
    pub error_count: u32,
}

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
    /// Monotonic counter bumped whenever the UI should imperatively scroll
    /// the stream container to the bottom. The Stream component watches
    /// this via a Leptos effect; any change triggers a DOM scroll.
    pub scroll_request_counter: u64,

    /// Visible apps. Empty set = no app filter applied (allow all).
    /// Non-empty = strict include-only-these-apps.
    pub app_set: BTreeSet<String>,
    /// Visible hosts. Same semantics as app_set.
    pub host_set: BTreeSet<String>,

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
            scroll_request_counter: 0,
            app_set: BTreeSet::new(),
            host_set: BTreeSet::new(),
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
    ///
    /// `rs` arrives **newest-first** (the server sorts descending and
    /// truncates to the newest `limit`); the stream renders front-to-back
    /// top-to-bottom, so we store ascending.
    pub fn replace_records(&mut self, rs: Vec<Record>) {
        let mut rs = rs;
        rs.reverse();
        if rs.len() > RECORD_CAP {
            rs.drain(..rs.len() - RECORD_CAP);
        }
        self.records.clear();
        self.records.extend(rs);
        self.pending.clear();
        self.pending_count = 0;
        self.selected_row_index = None;
    }

    /// Merge a backfill of historical records into the buffer.
    ///
    /// Called once at mount. `history` is **newest-first** from `/v1/search`.
    /// The live WebSocket may already have delivered records that overlap the
    /// fetched window (the request is issued concurrently with the socket
    /// opening), so this dedups on a natural key and re-sorts ascending
    /// rather than assuming the two ranges are disjoint.
    pub fn merge_backfill(&mut self, history: Vec<Record>) {
        let mut seen: HashSet<NaturalKey> = self.records.iter().map(natural_key).collect();

        let mut merged: Vec<Record> = Vec::with_capacity(history.len() + self.records.len());
        for r in history {
            if seen.insert(natural_key(&r)) {
                merged.push(r);
            }
        }
        merged.extend(self.records.drain(..));

        // Stable sort: a live record whose timestamp falls inside the history
        // range lands in its true position instead of being appended.
        merged.sort_by_key(|r| r.timestamp);

        if merged.len() > RECORD_CAP {
            merged.drain(..merged.len() - RECORD_CAP);
        }

        self.records.clear();
        self.records.extend(merged);
        // Row positions were reassigned wholesale.
        self.selected_row_index = None;
    }

    /// User explicitly resumed: drain + scroll + live on.
    /// Also signals the UI to scroll the stream container to the bottom
    /// (the Stream component watches `scroll_request_counter`).
    pub fn resume_live(&mut self) {
        self.drain_pending();
        self.live = true;
        self.at_bottom = true;
        self.scroll_request_counter = self.scroll_request_counter.wrapping_add(1);
    }

    /// Scroll-up detection: set live=false when we leave the bottom.
    pub fn on_scroll_not_at_bottom(&mut self) {
        self.live = false;
        self.at_bottom = false;
    }

    /// Scroll-to-bottom detection. If currently paused, auto-resume live
    /// (drain pending, set live, bump scroll-request counter). If already
    /// live, no-op except setting `at_bottom = true` — we're already there,
    /// no need to scroll the DOM again.
    pub fn on_scroll_to_bottom(&mut self) {
        if self.live {
            self.at_bottom = true;
        } else {
            self.resume_live();
        }
    }

    /// Filter helper: returns true when the app should be visible.
    /// Empty `app_set` = no filter (allow all). Non-empty = strict
    /// include-only.
    #[must_use]
    pub fn app_set_allows(&self, app: &str) -> bool {
        self.app_set.is_empty() || self.app_set.contains(app)
    }

    /// Same semantics for hosts.
    #[must_use]
    pub fn host_set_allows(&self, host: &str) -> bool {
        self.host_set.is_empty() || self.host_set.contains(host)
    }

    /// Toggle the inclusion of an app in `app_set` (insert if absent, remove if present).
    pub fn toggle_app(&mut self, app: &str) {
        if !self.app_set.remove(app) {
            self.app_set.insert(app.to_string());
        }
    }

    /// Toggle the inclusion of a host in `host_set`.
    pub fn toggle_host(&mut self, host: &str) {
        if !self.host_set.remove(host) {
            self.host_set.insert(host.to_string());
        }
    }

    /// Replace `app_set` wholesale. Used by the popover's all/none/invert
    /// footer actions.
    pub fn set_apps(&mut self, apps: impl IntoIterator<Item = String>) {
        self.app_set = apps.into_iter().collect();
    }

    /// Replace `host_set` wholesale.
    pub fn set_hosts(&mut self, hosts: impl IntoIterator<Item = String>) {
        self.host_set = hosts.into_iter().collect();
    }

    /// Bucket records by timestamp into `RATE_BUCKET_COUNT` equal-width
    /// time slots spanning `[from, to)`. Counts totals + warn + error
    /// per bucket. Records outside the range are skipped.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn rate_buckets(
        &self,
        from: chrono::DateTime<chrono::Utc>,
        to: chrono::DateTime<chrono::Utc>,
    ) -> Vec<RateBucket> {
        let mut buckets = vec![RateBucket::default(); RATE_BUCKET_COUNT];
        if to <= from {
            return buckets;
        }
        let span_ms = (to - from).num_milliseconds().max(1) as f64;
        let bucket_ms = span_ms / RATE_BUCKET_COUNT as f64;
        for r in &self.records {
            if r.timestamp < from || r.timestamp >= to {
                continue;
            }
            let offset_ms = (r.timestamp - from).num_milliseconds() as f64;
            let idx = (offset_ms / bucket_ms) as usize;
            let idx = idx.min(RATE_BUCKET_COUNT - 1);
            buckets[idx].total += 1;
            match r.level {
                Level::Warn => buckets[idx].warn_count += 1,
                Level::Error | Level::Fatal => buckets[idx].error_count += 1,
                _ => {}
            }
        }
        buckets
    }

    /// Filtered view: level + query substring + app set + host set.
    #[must_use]
    pub fn filtered(&self) -> Vec<&Record> {
        let q = self.query.as_str();
        self.records
            .iter()
            .filter(|r| self.level_set.allows(r.level))
            .filter(|r| q.is_empty() || r.body.contains(q))
            .filter(|r| self.app_set_allows(&r.app))
            .filter(|r| self.host_set_allows(&r.host))
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

/// Identity used to dedup a backfilled record against one already delivered
/// live. `Record` carries no id; two records agreeing on all three of these
/// are indistinguishable to a reader.
type NaturalKey = (DateTime<Utc>, String, String);

fn natural_key(r: &Record) -> NaturalKey {
    (r.timestamp, r.stream_id.clone(), r.body.clone())
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
    use std::collections::BTreeMap;
    use tracecure_core::{Level, Record};

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

    // --- scroll-to-bottom mechanism (bugs #1 + #2) ---

    #[test]
    fn resume_live_bumps_scroll_request_counter() {
        let mut s = AppState::new();
        let initial = s.scroll_request_counter;
        s.resume_live();
        assert_eq!(
            s.scroll_request_counter,
            initial + 1,
            "resume_live must signal a DOM scroll request"
        );
    }

    #[test]
    fn on_scroll_to_bottom_resumes_live_when_paused() {
        let mut s = AppState::new();
        s.live = false;
        s.at_bottom = false;
        s.push_record(rec("a", Level::Info));
        s.push_record(rec("b", Level::Info));
        assert_eq!(s.pending.len(), 2, "precondition: 2 pending while paused");
        let initial_counter = s.scroll_request_counter;

        s.on_scroll_to_bottom();

        assert!(
            s.live,
            "live should be true after scroll to bottom while paused"
        );
        assert!(s.at_bottom);
        assert_eq!(s.pending.len(), 0, "pending should drain into records");
        assert_eq!(s.records.len(), 2);
        assert_eq!(
            s.scroll_request_counter,
            initial_counter + 1,
            "scroll request counter should bump"
        );
    }

    #[test]
    fn on_scroll_to_bottom_already_live_does_not_bump_counter() {
        let mut s = AppState::new();
        assert!(s.live, "default state is live");
        let initial = s.scroll_request_counter;

        s.on_scroll_to_bottom();

        assert!(s.live);
        assert!(s.at_bottom);
        assert_eq!(
            s.scroll_request_counter, initial,
            "no scroll bump when already live - we're already at bottom"
        );
    }

    // --- backfill merge (blank-on-remount fix) ---

    /// Build a record at `base + secs` with a distinguishing body.
    fn rec_at(secs: i64, body: &str) -> Record {
        let base = Utc.with_ymd_and_hms(2026, 7, 19, 9, 0, 0).unwrap();
        let mut r = rec(body, Level::Info);
        r.timestamp = base + chrono::Duration::seconds(secs);
        r
    }

    fn bodies(s: &AppState) -> Vec<String> {
        s.records.iter().map(|r| r.body.clone()).collect()
    }

    #[test]
    fn merge_backfill_reverses_newest_first_into_ascending() {
        let mut s = AppState::new();
        // Server order: newest first.
        s.merge_backfill(vec![rec_at(30, "c"), rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(
            bodies(&s),
            vec!["a", "b", "c"],
            "history must render oldest-at-top to match the live append order"
        );
    }

    #[test]
    fn merge_backfill_dedups_against_records_already_delivered_live() {
        let mut s = AppState::new();
        // A record arrived over the WS before the backfill response landed.
        s.push_record(rec_at(30, "c"));
        // The backfill window covers it too.
        s.merge_backfill(vec![rec_at(30, "c"), rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(
            bodies(&s),
            vec!["a", "b", "c"],
            "the overlapping record must appear exactly once"
        );
    }

    #[test]
    fn merge_backfill_interleaves_a_live_record_inside_the_history_range() {
        let mut s = AppState::new();
        // Late-ingested / clock-skewed record whose timestamp sits mid-history.
        s.push_record(rec_at(15, "mid"));
        s.merge_backfill(vec![rec_at(30, "c"), rec_at(10, "a")]);
        assert_eq!(
            bodies(&s),
            vec!["a", "mid", "c"],
            "sort must place it by timestamp, not append it"
        );
    }

    #[test]
    fn merge_backfill_caps_to_record_cap_keeping_the_newest() {
        let mut s = AppState::new();
        let history: Vec<Record> = (0..(RECORD_CAP as i64 + 100))
            .rev() // newest-first, as the server sends
            .map(|i| rec_at(i, &format!("r{i}")))
            .collect();
        s.merge_backfill(history);
        assert_eq!(s.records.len(), RECORD_CAP);
        assert_eq!(
            s.records.back().unwrap().body,
            format!("r{}", RECORD_CAP + 99),
            "the newest record must survive"
        );
        assert_eq!(
            s.records.front().unwrap().body,
            format!("r{}", 100),
            "the oldest 100 must be dropped"
        );
    }

    #[test]
    fn merge_backfill_clears_selection() {
        let mut s = AppState::new();
        s.push_record(rec_at(30, "c"));
        s.open_detail(0);
        assert_eq!(s.selected_row_index, Some(0));
        s.merge_backfill(vec![rec_at(10, "a")]);
        assert_eq!(
            s.selected_row_index, None,
            "row positions were reassigned, so the old index is meaningless"
        );
    }

    #[test]
    fn merge_backfill_into_an_empty_buffer_is_just_the_history() {
        let mut s = AppState::new();
        assert!(s.records.is_empty());
        s.merge_backfill(vec![rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(bodies(&s), vec!["a", "b"]);
    }

    #[test]
    fn replace_records_reverses_to_ascending() {
        let mut s = AppState::new();
        s.replace_records(vec![rec_at(30, "c"), rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(bodies(&s), vec!["a", "b", "c"]);
    }

    // --- app/host facet filters ---

    #[test]
    fn app_set_allows_returns_true_when_set_is_empty() {
        let s = AppState::new();
        assert!(s.app_set.is_empty(), "default app_set should be empty");
        assert!(
            s.app_set_allows("anything"),
            "empty app_set means no filter - must allow all"
        );
    }

    #[test]
    fn app_set_allows_returns_true_when_app_in_set() {
        let mut s = AppState::new();
        s.app_set.insert("fanid_app".to_string());
        assert!(s.app_set_allows("fanid_app"));
    }

    #[test]
    fn app_set_allows_returns_false_when_app_not_in_set() {
        let mut s = AppState::new();
        s.app_set.insert("fanid_app".to_string());
        assert!(!s.app_set_allows("fanid_admin"));
    }

    #[test]
    fn toggle_app_inserts_then_removes() {
        let mut s = AppState::new();
        assert!(s.app_set.is_empty());
        s.toggle_app("fanid_app");
        assert!(s.app_set.contains("fanid_app"));
        s.toggle_app("fanid_app");
        assert!(!s.app_set.contains("fanid_app"));
    }

    #[test]
    fn toggle_host_inserts_then_removes() {
        let mut s = AppState::new();
        s.toggle_host("KINGS_PC");
        assert!(s.host_set.contains("KINGS_PC"));
        s.toggle_host("KINGS_PC");
        assert!(!s.host_set.contains("KINGS_PC"));
    }

    #[test]
    fn set_apps_replaces_existing_set() {
        let mut s = AppState::new();
        s.toggle_app("old");
        assert!(s.app_set.contains("old"));
        s.set_apps(["fanid_app".to_string(), "fanid_admin".to_string()]);
        assert!(!s.app_set.contains("old"));
        assert!(s.app_set.contains("fanid_app"));
        assert!(s.app_set.contains("fanid_admin"));
        assert_eq!(s.app_set.len(), 2);
    }

    #[test]
    fn set_hosts_replaces_existing_set() {
        let mut s = AppState::new();
        s.toggle_host("old");
        s.set_hosts(["KINGS_PC".to_string()]);
        assert!(!s.host_set.contains("old"));
        assert!(s.host_set.contains("KINGS_PC"));
    }

    #[test]
    fn rate_buckets_distributes_evenly() {
        use chrono::TimeZone;
        let mut s = AppState::new();
        let base = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 0).unwrap();
        for i in 0..30 {
            let mut r = rec("x", Level::Info);
            r.timestamp = base + chrono::Duration::minutes(i);
            s.records.push_back(r);
        }
        let buckets = s.rate_buckets(base, base + chrono::Duration::minutes(30));
        assert_eq!(buckets.len(), 30);
        let total: u32 = buckets.iter().map(|b| b.total).sum();
        assert_eq!(total, 30, "all 30 records should land in buckets");
    }

    #[test]
    fn rate_buckets_tints_by_level() {
        use chrono::TimeZone;
        let mut s = AppState::new();
        let base = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 0).unwrap();
        let mut r1 = rec("e", Level::Error);
        r1.timestamp = base + chrono::Duration::minutes(5);
        let mut r2 = rec("w", Level::Warn);
        r2.timestamp = base + chrono::Duration::minutes(5);
        s.records.push_back(r1);
        s.records.push_back(r2);
        let buckets = s.rate_buckets(base, base + chrono::Duration::minutes(30));
        let bucket_5 = &buckets[5];
        assert_eq!(bucket_5.total, 2);
        assert_eq!(bucket_5.error_count, 1);
        assert_eq!(bucket_5.warn_count, 1);
    }

    #[test]
    fn rate_buckets_excludes_out_of_range() {
        use chrono::TimeZone;
        let mut s = AppState::new();
        let base = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 0).unwrap();
        let mut earlier = rec("x", Level::Info);
        earlier.timestamp = base - chrono::Duration::minutes(1);
        let mut later = rec("x", Level::Info);
        later.timestamp = base + chrono::Duration::minutes(31);
        s.records.push_back(earlier);
        s.records.push_back(later);
        let buckets = s.rate_buckets(base, base + chrono::Duration::minutes(30));
        let total: u32 = buckets.iter().map(|b| b.total).sum();
        assert_eq!(total, 0, "out-of-range records must be skipped");
    }

    #[test]
    fn filtered_combines_level_app_host_query() {
        use chrono::TimeZone;
        let mut s = AppState::new();
        let mut r1 = rec("hello world", Level::Info);
        r1.timestamp = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 0).unwrap();
        r1.app = "fanid_app".into();
        r1.host = "KINGS_PC".into();
        let mut r2 = rec("hello fanid_admin", Level::Info);
        r2.timestamp = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 1).unwrap();
        r2.app = "fanid_admin".into();
        r2.host = "prod-01".into();
        s.records.push_back(r1);
        s.records.push_back(r2);

        // Default state: no filter → both visible.
        assert_eq!(s.filtered().len(), 2);

        // app_set = {fanid_app} → only r1.
        s.toggle_app("fanid_app");
        let f = s.filtered();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].app, "fanid_app");

        // Add host filter that excludes the only remaining record.
        s.toggle_host("prod-01");
        assert_eq!(s.filtered().len(), 0);
    }
}
