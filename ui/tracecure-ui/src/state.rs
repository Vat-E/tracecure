//! `AppState` — plain Rust, no Leptos. The shell component wraps an
//! `AppState` in an `RwSignal` and every component that reads/writes
//! state goes through that.
//!
//! Methods mirror the TUI's `AppState` (crates/tracecure-cli/src/tui/state.rs)
//! wherever semantics match, so the mental model carries over.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tracecure_core::{Level, Record};

use crate::command::CommandAction;

pub const RECORD_CAP: usize = 5000;
pub const RATE_BUCKET_COUNT: usize = 30;

/// A buffered record plus a stable identity.
///
/// `seq` is assigned once at insert, never reused, and survives ring
/// eviction. It exists because the stream's `<For>` needs a key that
/// denotes *this record* rather than its current position: keying on the
/// positional index means that once the buffer reaches `RECORD_CAP` and
/// every push does `pop_front` + `push_back`, the key set is unchanged
/// (`{0..CAP-1}`) even though every record shifted a slot. A keyed
/// reconciler correctly concludes there is nothing to do, and the rendered
/// rows silently detach from the underlying data.
///
/// `rec` is an `Arc` so cloning the filtered view is a refcount bump rather
/// than a deep copy of four `String`s and two `BTreeMap`s per record.
/// (`Arc` not `Rc`: Leptos bounds `RwSignal<T>` on `T: Send + Sync`. On
/// wasm32 without atomics the refcount ops lower to plain loads/stores, so
/// it costs the same as `Rc` here.)
// Not `Eq`: `Record::fields` holds `serde_json::Value`, which is only
// `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub seq: u64,
    pub rec: Arc<Record>,
}

impl Entry {
    #[must_use]
    pub fn new(seq: u64, rec: Record) -> Self {
        Self {
            seq,
            rec: Arc::new(rec),
        }
    }
}

/// Which row is selected and whether its detail pane is open.
///
/// Split out of `AppState` into its own signal: every mounted `Row` reads
/// this, and a single coarse signal meant an arriving log record woke all
/// ~3 closures in all ~5000 rows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Selection {
    /// Identified by `Entry::seq`, not row index, so the selection tracks
    /// the record across eviction and filter changes.
    pub selected_seq: Option<u64>,
    pub detail_expanded: bool,
}

impl Selection {
    #[must_use]
    pub fn is_selected(&self, seq: u64) -> bool {
        self.selected_seq == Some(seq)
    }

    #[must_use]
    pub fn is_expanded(&self, seq: u64) -> bool {
        self.detail_expanded && self.is_selected(seq)
    }

    /// Click behaviour: clicking the open row closes it, anything else opens.
    pub fn toggle(&mut self, seq: u64) {
        if self.is_expanded(seq) {
            self.close();
        } else {
            self.open(seq);
        }
    }

    pub fn open(&mut self, seq: u64) {
        self.selected_seq = Some(seq);
        self.detail_expanded = true;
    }

    pub fn close(&mut self) {
        self.detail_expanded = false;
    }

    pub fn clear(&mut self) {
        self.selected_seq = None;
        self.detail_expanded = false;
    }
}

/// Presentation options every row reads. Its own signal for the same
/// reason as `Selection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOpts {
    pub timestamp_mode: TimestampMode,
}

impl Default for ViewOpts {
    fn default() -> Self {
        Self {
            timestamp_mode: TimestampMode::Absolute,
        }
    }
}

impl ViewOpts {
    pub fn toggle_timestamp_mode(&mut self) {
        self.timestamp_mode = match self.timestamp_mode {
            TimestampMode::Absolute => TimestampMode::Relative,
            TimestampMode::Relative => TimestampMode::Absolute,
        };
    }
}

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
    pub records: VecDeque<Entry>,
    pub pending: VecDeque<Entry>,
    pub pending_count: usize,
    pub status: Status,
    /// Monotonic source for `Entry::seq`. Never reset.
    next_seq: u64,

    // View
    pub time_window: TimeWindow,
    pub level_set: LevelSet,
    pub query: String,

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
            pending: VecDeque::new(),
            pending_count: 0,
            status: Status::Disconnected,
            next_seq: 0,
            time_window: TimeWindow::default_1h(),
            level_set: LevelSet::defaults(),
            query: String::new(),
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

    /// Allocate the next stable sequence number.
    fn take_seq(&mut self) -> u64 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// Push a record. If paused, it goes to `pending`. Caller is the
    /// WebSocket callback.
    pub fn push_record(&mut self, r: Record) {
        let entry = Entry::new(self.take_seq(), r);
        if self.live && self.at_bottom {
            if self.records.len() >= RECORD_CAP {
                self.records.pop_front();
            }
            self.records.push_back(entry);
        } else {
            // Bounded for the same reason `records` is: a user who scrolls
            // up and walks away would otherwise accumulate without limit for
            // as long as the tab stays open, and `drain_pending` would throw
            // all but the newest RECORD_CAP away anyway.
            if self.pending.len() >= RECORD_CAP {
                self.pending.pop_front();
            }
            self.pending.push_back(entry);
            self.pending_count = self.pending.len();
        }
    }

    /// Push a batch of records in one pass. Used by the WebSocket flush so a
    /// burst costs one state notification instead of one per record.
    /// NOTE: this deliberately does *not* request a scroll-to-bottom, even
    /// though a live tail arguably should follow the newest record. Doing so
    /// per batch (`scrollTop = scrollHeight` up to 60×/s against a container
    /// whose rows use `content-visibility`) forced a layout each frame and
    /// froze the renderer outright under load — measurably worse than the
    /// drift it was meant to fix. Following the bottom needs to be driven by
    /// user-intent events (wheel/drag/keys) rather than by scroll position;
    /// see the follow-ups note.
    pub fn push_batch(&mut self, rs: Vec<Record>) {
        for r in rs {
            self.push_record(r);
        }
    }

    /// Drain pending → records and clear counter. Called on resume.
    pub fn drain_pending(&mut self) {
        for e in std::mem::take(&mut self.pending) {
            if self.records.len() >= RECORD_CAP {
                self.records.pop_front();
            }
            self.records.push_back(e);
        }
        self.pending_count = 0;
    }

    /// Replace records wholesale (e.g. after a windowed search refetch).
    ///
    /// `rs` arrives **newest-first** (the server sorts descending and
    /// truncates to the newest `limit`); the stream renders front-to-back
    /// top-to-bottom, so we store ascending.
    pub fn replace_records(&mut self, rs: Vec<Record>) {
        // Stable sort rather than `reverse()`: identical under the server's
        // documented ordering, but tie-correct for records sharing a
        // timestamp, and it matches what `merge_backfill` does. `reverse()`
        // would flip same-timestamp groups relative to the other path.
        let mut rs = rs;
        rs.sort_by_key(|r| r.timestamp);
        if rs.len() > RECORD_CAP {
            rs.drain(..rs.len() - RECORD_CAP);
        }
        let had_any = !rs.is_empty();
        self.records.clear();
        for r in rs {
            let seq = self.take_seq();
            self.records.push_back(Entry::new(seq, r));
        }
        self.pending.clear();
        self.pending_count = 0;
        // Newest is now at the bottom; without this the freshest result sits
        // below the fold and the view looks stuck on the oldest match.
        if had_any {
            self.request_scroll_to_bottom();
        }
    }

    /// Merge a backfill of historical records into the buffer.
    ///
    /// Called once at mount. `history` is **newest-first** from `/v1/search`.
    /// The live WebSocket may already have delivered records that overlap the
    /// fetched window (the request is issued concurrently with the socket
    /// opening), so this dedups on a natural key and re-sorts ascending
    /// rather than assuming the two ranges are disjoint.
    pub fn merge_backfill(&mut self, history: Vec<Record>) {
        // Count — not just presence — of what the live socket already
        // delivered, including anything held in `pending` while paused.
        // Multiplicity matters: a service that logs the same line twice in
        // the same millisecond emits two genuinely distinct records, and a
        // set-based dedup would silently drop one on every load.
        let mut already: HashMap<NaturalKey, usize> = HashMap::new();
        for e in self.records.iter().chain(self.pending.iter()) {
            *already.entry(natural_key(&e.rec)).or_insert(0) += 1;
        }

        let mut merged: Vec<Entry> = Vec::with_capacity(history.len() + self.records.len());
        for r in history {
            // Drop this history record only if it accounts for one of the
            // copies we already hold.
            if let Some(n) = already.get_mut(&natural_key(&r)) {
                if *n > 0 {
                    *n -= 1;
                    continue;
                }
            }
            let seq = self.take_seq();
            merged.push(Entry::new(seq, r));
        }
        // Records already delivered live keep their seq, so a row the user
        // has open stays open across the backfill.
        merged.extend(self.records.drain(..));

        // Stable sort: a live record whose timestamp falls inside the history
        // range lands in its true position instead of being appended.
        merged.sort_by_key(|e| e.rec.timestamp);

        if merged.len() > RECORD_CAP {
            merged.drain(..merged.len() - RECORD_CAP);
        }

        let had_any = !merged.is_empty();
        self.records.clear();
        self.records.extend(merged);

        // History is oldest-at-top, so a fresh load would otherwise sit
        // pinned at the oldest record with the live tail below the fold —
        // looking frozen on hour-old logs with no paused banner to explain
        // it. Only when the user has not already scrolled away.
        if had_any && self.live && self.at_bottom {
            self.request_scroll_to_bottom();
        }
    }

    /// Ask the stream to scroll to the bottom on the next render.
    fn request_scroll_to_bottom(&mut self) {
        self.scroll_request_counter = self.scroll_request_counter.wrapping_add(1);
    }

    /// User explicitly resumed: drain + scroll + live on.
    /// Also signals the UI to scroll the stream container to the bottom
    /// (the Stream component watches `scroll_request_counter`).
    pub fn resume_live(&mut self) {
        self.drain_pending();
        self.live = true;
        self.at_bottom = true;
        self.request_scroll_to_bottom();
    }

    /// Scroll-up detection: set live=false when we leave the bottom.
    pub fn on_scroll_not_at_bottom(&mut self) {
        self.live = false;
        self.at_bottom = false;
    }

    /// Would `on_scroll(at_bottom)` change anything?
    ///
    /// The scroll handler fires roughly once per animation frame while
    /// dragging, and a signal write notifies every subscriber whether or not
    /// the value changed. Checking first keeps a drag from broadcasting a
    /// state change it isn't making.
    #[must_use]
    pub fn scroll_would_change(&self, at_bottom: bool) -> bool {
        if at_bottom {
            // Reaching the bottom while paused resumes live (a real change);
            // reporting the bottom while already live and at_bottom is a no-op.
            !self.live || !self.at_bottom
        } else {
            self.live || self.at_bottom
        }
    }

    /// Apply a scroll position report.
    pub fn on_scroll(&mut self, at_bottom: bool) {
        if at_bottom {
            self.on_scroll_to_bottom();
        } else {
            self.on_scroll_not_at_bottom();
        }
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
        for e in &self.records {
            let r = &e.rec;
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
    ///
    /// Returns owned `Entry`s, which is an `Arc` refcount bump per record —
    /// not a deep clone. This runs on every render of the stream.
    #[must_use]
    pub fn filtered(&self) -> Vec<Entry> {
        let q = self.query.as_str();
        self.records
            .iter()
            .filter(|e| self.level_set.allows(e.rec.level))
            .filter(|e| q.is_empty() || e.rec.body.contains(q))
            .filter(|e| self.app_set_allows(&e.rec.app))
            .filter(|e| self.host_set_allows(&e.rec.host))
            .cloned()
            .collect()
    }

    pub fn toggle_level(&mut self, lvl: Level) {
        self.level_set.toggle(lvl);
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

    /// Is there a flash that has actually expired?
    ///
    /// The flash ticker polls twice a second for the lifetime of the page.
    /// Checking this before writing keeps an idle tab from broadcasting a
    /// state change 2×/s — which, with a full buffer, re-ran the stream's
    /// filter and diff for nothing.
    #[must_use]
    pub fn flash_is_due(&self) -> bool {
        self.flash
            .as_ref()
            .is_some_and(|f| now_ms() >= f.expires_at_ms)
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
        assert_eq!(s.filtered()[0].rec.body, "info msg");
    }

    #[test]
    fn filtered_respects_query_substring() {
        let mut s = AppState::new();
        s.push_record(rec("hello world", Level::Info));
        s.push_record(rec("goodbye", Level::Info));
        s.query = "hello".into();
        let f = s.filtered();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rec.body, "hello world");
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
        let mut v = ViewOpts::default();
        assert_eq!(v.timestamp_mode, TimestampMode::Absolute);
        v.toggle_timestamp_mode();
        assert_eq!(v.timestamp_mode, TimestampMode::Relative);
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
    fn open_detail_sets_seq_and_expanded() {
        let mut sel = Selection::default();
        sel.open(7);
        assert_eq!(sel.selected_seq, Some(7));
        assert!(sel.detail_expanded);
        assert!(sel.is_expanded(7));
        assert!(!sel.is_expanded(8));
        sel.close();
        assert!(!sel.detail_expanded);
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

    // --- stable identity + cheap clones (lag fix) ---

    /// The bug this exists to prevent: at `RECORD_CAP`, `pop_front` +
    /// `push_back` leaves the *index* set unchanged, so an index-keyed
    /// reconciler sees no diff and stops updating the view. Sequence
    /// numbers keep moving, so the diff stays truthful.
    #[test]
    fn seq_keeps_advancing_after_the_ring_starts_evicting() {
        let mut s = AppState::new();
        for _ in 0..RECORD_CAP {
            s.push_record(rec("x", Level::Info));
        }
        let keys_before: Vec<u64> = s.records.iter().map(|e| e.seq).collect();

        s.push_record(rec("newest", Level::Info));

        let keys_after: Vec<u64> = s.records.iter().map(|e| e.seq).collect();
        assert_eq!(s.records.len(), RECORD_CAP, "still capped");
        assert_ne!(
            keys_before, keys_after,
            "the key set must change when the contents shift, or the \
             reconciler has no way to know anything happened"
        );
        assert_eq!(s.records.back().unwrap().rec.body, "newest");
    }

    #[test]
    fn seq_is_strictly_increasing_and_never_reused() {
        let mut s = AppState::new();
        for _ in 0..(RECORD_CAP + 10) {
            s.push_record(rec("x", Level::Info));
        }
        let seqs: Vec<u64> = s.records.iter().map(|e| e.seq).collect();
        assert!(
            seqs.windows(2).all(|w| w[0] < w[1]),
            "seqs must be strictly increasing"
        );
        assert_eq!(seqs[0], 10, "the first 10 were evicted, not renumbered");
    }

    /// `filtered()` runs on every stream render; it must not deep-copy the
    /// records. Pointer equality proves the `Arc` is shared, not cloned.
    #[test]
    fn filtered_shares_records_instead_of_deep_copying_them() {
        let mut s = AppState::new();
        s.push_record(rec("a", Level::Info));
        let f = s.filtered();
        assert!(
            Arc::ptr_eq(&f[0].rec, &s.records[0].rec),
            "filtered() must hand out shared handles, not clones"
        );
    }

    #[test]
    fn selection_by_seq_survives_eviction_of_other_records() {
        let mut s = AppState::new();
        s.push_record(rec("keep-me", Level::Info));
        let seq = s.records[0].seq;
        let mut sel = Selection::default();
        sel.open(seq);

        // Push enough to shift, but not enough to evict it.
        for _ in 0..10 {
            s.push_record(rec("x", Level::Info));
        }

        let still = s.records.iter().find(|e| sel.is_selected(e.seq));
        assert_eq!(
            still.map(|e| e.rec.body.as_str()),
            Some("keep-me"),
            "selection must follow the record, not the row position"
        );
    }

    #[test]
    fn pending_is_capped_at_record_cap() {
        let mut s = AppState::new();
        s.live = false;
        s.at_bottom = false;
        for i in 0..(RECORD_CAP + 100) {
            s.push_record(rec(&format!("r{i}"), Level::Info));
        }
        assert_eq!(
            s.pending.len(),
            RECORD_CAP,
            "a paused tab must not accumulate without bound"
        );
        assert_eq!(
            s.pending.back().unwrap().rec.body,
            format!("r{}", RECORD_CAP + 99),
            "the newest must be the ones retained"
        );
    }

    /// Regression guard for the renderer freeze: requesting a scroll per
    /// batch forced a layout up to 60×/s against a `content-visibility`
    /// container and locked the main thread under load.
    #[test]
    fn push_batch_does_not_request_a_scroll() {
        let mut s = AppState::new();
        let before = s.scroll_request_counter;
        s.push_batch(vec![rec("a", Level::Info), rec("b", Level::Info)]);
        assert_eq!(s.records.len(), 2);
        assert_eq!(
            s.scroll_request_counter, before,
            "scrolling per batch froze the renderer; see push_batch docs"
        );
    }

    #[test]
    fn push_batch_while_paused_buffers_into_pending() {
        let mut s = AppState::new();
        s.on_scroll(false);
        s.push_batch(vec![rec("a", Level::Info)]);
        assert_eq!(s.pending.len(), 1);
        assert!(s.records.is_empty());
    }

    #[test]
    fn push_batch_matches_repeated_push_record() {
        let mut a = AppState::new();
        let mut b = AppState::new();
        let rs = vec![rec("1", Level::Info), rec("2", Level::Info)];
        for r in rs.clone() {
            a.push_record(r);
        }
        b.push_batch(rs);
        assert_eq!(bodies(&a), bodies(&b));
        assert_eq!(
            a.records.iter().map(|e| e.seq).collect::<Vec<_>>(),
            b.records.iter().map(|e| e.seq).collect::<Vec<_>>()
        );
    }

    // --- redundant-write guards (lag fix) ---

    #[test]
    fn scroll_at_bottom_while_already_live_is_a_no_op() {
        let s = AppState::new();
        assert!(s.live && s.at_bottom, "precondition: default is live");
        assert!(
            !s.scroll_would_change(true),
            "reporting the bottom we are already at must not notify anyone"
        );
    }

    #[test]
    fn scrolling_away_from_the_bottom_is_a_real_change_once() {
        let mut s = AppState::new();
        assert!(s.scroll_would_change(false), "first scroll away pauses");
        s.on_scroll(false);
        assert!(!s.live);
        assert!(
            !s.scroll_would_change(false),
            "every later frame of the same drag must be a no-op"
        );
    }

    #[test]
    fn scrolling_back_to_the_bottom_while_paused_is_a_real_change() {
        let mut s = AppState::new();
        s.on_scroll(false);
        assert!(s.scroll_would_change(true), "returning to bottom resumes");
        s.on_scroll(true);
        assert!(s.live && s.at_bottom);
    }

    #[test]
    fn flash_is_not_due_when_there_is_no_flash() {
        let s = AppState::new();
        assert!(s.flash.is_none());
        assert!(
            !s.flash_is_due(),
            "the idle case must not trigger a state write twice a second"
        );
    }

    #[test]
    fn flash_is_not_due_before_it_expires() {
        let mut s = AppState::new();
        s.set_flash_ok("hi");
        assert!(!s.flash_is_due(), "a fresh flash has ~2s to live");
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
        s.records.iter().map(|e| e.rec.body.clone()).collect()
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
            s.records.back().unwrap().rec.body,
            format!("r{}", RECORD_CAP + 99),
            "the newest record must survive"
        );
        assert_eq!(
            s.records.front().unwrap().rec.body,
            format!("r{}", 100),
            "the oldest 100 must be dropped"
        );
    }

    /// Selection is by `seq`, so it survives the wholesale reordering a
    /// backfill performs — this is the payoff for not keying on row index.
    #[test]
    fn merge_backfill_preserves_a_live_records_identity() {
        let mut s = AppState::new();
        s.push_record(rec_at(30, "c"));
        let live_seq = s.records[0].seq;
        s.merge_backfill(vec![rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(bodies(&s), vec!["a", "b", "c"]);
        let moved = s.records.iter().find(|e| e.seq == live_seq).unwrap();
        assert_eq!(
            moved.rec.body, "c",
            "the live record keeps its seq after being re-sorted"
        );
    }

    /// History renders oldest-at-top, so without a scroll request the user
    /// lands on the oldest record with the live tail below the fold — the
    /// page looks frozen on old logs and no paused banner explains it.
    #[test]
    fn merge_backfill_requests_a_scroll_to_the_bottom() {
        let mut s = AppState::new();
        let before = s.scroll_request_counter;
        s.merge_backfill(vec![rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(
            s.scroll_request_counter,
            before + 1,
            "a populated backfill must scroll the viewport to the newest record"
        );
    }

    #[test]
    fn merge_backfill_does_not_yank_a_user_who_scrolled_away() {
        let mut s = AppState::new();
        s.on_scroll(false); // user scrolled up while the fetch was in flight
        let before = s.scroll_request_counter;
        s.merge_backfill(vec![rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(
            s.scroll_request_counter, before,
            "must not jump the viewport out from under a paused reader"
        );
    }

    #[test]
    fn merge_backfill_of_nothing_does_not_request_a_scroll() {
        let mut s = AppState::new();
        let before = s.scroll_request_counter;
        s.merge_backfill(vec![]);
        assert_eq!(s.scroll_request_counter, before);
    }

    #[test]
    fn replace_records_requests_a_scroll_to_the_bottom() {
        let mut s = AppState::new();
        let before = s.scroll_request_counter;
        s.replace_records(vec![rec_at(20, "b"), rec_at(10, "a")]);
        assert_eq!(
            s.scroll_request_counter,
            before + 1,
            "storage is ascending now, so the newest hit is at the bottom"
        );
    }

    /// Two genuinely distinct log lines can share a timestamp, stream and
    /// body — a service logging the same line twice in one millisecond. A
    /// set-based dedup would silently discard one on every page load.
    #[test]
    fn merge_backfill_keeps_genuine_duplicates_within_the_history() {
        let mut s = AppState::new();
        s.merge_backfill(vec![rec_at(10, "same"), rec_at(10, "same")]);
        assert_eq!(
            s.records.len(),
            2,
            "identical-but-distinct records must both survive"
        );
    }

    /// ...while still cancelling exactly the copies the socket already gave us.
    #[test]
    fn merge_backfill_cancels_only_as_many_copies_as_it_already_holds() {
        let mut s = AppState::new();
        s.push_record(rec_at(10, "same")); // one arrived live
        s.merge_backfill(vec![rec_at(10, "same"), rec_at(10, "same")]);
        assert_eq!(
            s.records.len(),
            2,
            "one history copy cancels the live one; the other is a real second record"
        );
    }

    #[test]
    fn merge_backfill_dedups_against_pending_while_paused() {
        let mut s = AppState::new();
        s.on_scroll(false); // paused: live records land in `pending`
        s.push_record(rec_at(30, "c"));
        assert_eq!(s.pending.len(), 1);

        s.merge_backfill(vec![rec_at(30, "c"), rec_at(10, "a")]);

        let in_records = s.records.iter().filter(|e| e.rec.body == "c").count();
        assert_eq!(
            in_records, 0,
            "the copy held in pending must suppress the history copy, or it \
             renders twice as soon as the user resumes"
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
            s.records.push_back(Entry::new(0, r));
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
        s.records.push_back(Entry::new(1, r1));
        s.records.push_back(Entry::new(2, r2));
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
        s.records.push_back(Entry::new(1, earlier));
        s.records.push_back(Entry::new(2, later));
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
        s.records.push_back(Entry::new(1, r1));
        s.records.push_back(Entry::new(2, r2));

        // Default state: no filter → both visible.
        assert_eq!(s.filtered().len(), 2);

        // app_set = {fanid_app} → only r1.
        s.toggle_app("fanid_app");
        let f = s.filtered();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rec.app, "fanid_app");

        // Add host filter that excludes the only remaining record.
        s.toggle_host("prod-01");
        assert_eq!(s.filtered().len(), 0);
    }
}
