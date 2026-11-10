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
use crate::filter::{Filter, ServerFilter, ALL_LEVELS};

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
    /// Move the selection without changing whether the detail pane is open,
    /// so stepping through rows with the keyboard carries an open pane along.
    pub fn select(&mut self, seq: u64) {
        self.selected_seq = Some(seq);
    }

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

    /// Human label: `Last 6 h` for a preset, `04-24 12:00 → 04-24 13:00 UTC`
    /// for a fixed range. One implementation, so the top bar, the status bar
    /// and the empty state cannot disagree about which window is showing.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Preset(d) => match d.as_secs() {
                900 => "Last 15 min".to_string(),
                3600 => "Last 1 h".to_string(),
                21_600 => "Last 6 h".to_string(),
                86_400 => "Last 24 h".to_string(),
                604_800 => "Last 7 d".to_string(),
                secs if secs % 3600 == 0 => format!("Last {} h", secs / 3600),
                secs if secs % 60 == 0 => format!("Last {} min", secs / 60),
                secs => format!("Last {secs} s"),
            },
            Self::Custom { from, to } => format!(
                "{} → {} UTC",
                from.format("%m-%d %H:%M"),
                to.format("%m-%d %H:%M")
            ),
        }
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

/// Where the view should land once a fetched buffer is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchAnchor {
    /// The newest record — a search or a new time range.
    Newest,
    /// The oldest record, paused — "load more" was asked for in order to
    /// read further back, so jumping to the tail would undo the request.
    Oldest,
}

/// What the record buffer was last fetched from the server with.
///
/// The buffer is a window onto the store, not the store: it holds at most
/// `limit` of the newest records that `filter` accepts. The UI needs both
/// facts to be honest about what a local filter is and is not searching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    /// What the server was asked to match: text, levels, app, host, stream.
    pub filter: ServerFilter,
    /// The `limit` the fetch was issued with.
    pub limit: usize,
    /// The server returned `limit` records, so older ones matching the
    /// request very likely exist and were cut off.
    pub truncated: bool,
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
    /// Raw text of the query box. Written only through `set_query`, which
    /// keeps `filter` in step.
    query: String,
    filter: Filter,
    pub loaded: Loaded,
    /// Generation of the newest window fetch. A response carrying an older
    /// one lost a race with a later request and is discarded.
    fetch_gen: u64,
    pub loading: bool,

    // Live / paused
    pub live: bool,
    pub at_bottom: bool,
    /// Monotonic counter bumped whenever the UI should imperatively scroll
    /// the stream container to the bottom. The Stream component watches
    /// this via a Leptos effect; any change triggers a DOM scroll.
    pub scroll_request_counter: u64,
    /// The same, for scrolling to the top (the oldest loaded record).
    pub scroll_top_request_counter: u64,

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
            filter: Filter::default(),
            loaded: Loaded::default(),
            fetch_gen: 0,
            loading: false,
            live: true,
            at_bottom: true,
            scroll_request_counter: 0,
            scroll_top_request_counter: 0,
            app_set: BTreeSet::new(),
            host_set: BTreeSet::new(),
            flash: None,
            command_open: false,
            command_input: String::new(),
            pending_command: None,
            help_open: false,
        }
    }

    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    #[must_use]
    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// Set the query box text and re-derive the filter from it.
    pub fn set_query(&mut self, q: impl Into<String>) {
        self.query = q.into();
        self.filter = Filter::parse(&self.query);
    }

    /// What a fetch would ask the server for right now: the query box
    /// combined with the level buttons.
    #[must_use]
    pub fn server_filter(&self) -> ServerFilter {
        let enabled: Vec<Level> = ALL_LEVELS
            .into_iter()
            .filter(|l| self.level_set.allows(*l))
            .collect();
        ServerFilter::new(&self.filter, &enabled)
    }

    /// True when what is on screen was loaded for a different request than
    /// the one the controls now describe — the view is filtering a buffer
    /// that only a server fetch can make complete. False until something has
    /// been loaded at all: there is no stale buffer to warn about yet.
    #[must_use]
    pub fn needs_server_search(&self) -> bool {
        self.loaded.limit != 0 && self.server_filter() != self.loaded.filter
    }

    /// Start a window fetch. Returns the generation to hand back to
    /// `finish_fetch` / `fail_fetch`.
    pub fn begin_fetch(&mut self) -> u64 {
        self.fetch_gen = self.fetch_gen.wrapping_add(1);
        self.loading = true;
        self.fetch_gen
    }

    /// Install the result of a window fetch. Returns `false` — and changes
    /// nothing — if a newer fetch has been started since, so a slow response
    /// cannot overwrite the result of the request the user made after it.
    pub fn finish_fetch(
        &mut self,
        gen: u64,
        filter: impl Into<ServerFilter>,
        limit: usize,
        rs: Vec<Record>,
        anchor: FetchAnchor,
    ) -> bool {
        if gen != self.fetch_gen {
            return false;
        }
        self.loading = false;
        self.loaded = Loaded {
            filter: filter.into(),
            limit,
            truncated: limit > 0 && rs.len() >= limit,
        };
        let before = self.scroll_request_counter;
        self.replace_records(rs);
        if anchor == FetchAnchor::Oldest {
            // Take back the scroll-to-bottom `replace_records` asked for.
            self.scroll_request_counter = before;
            self.live = false;
            self.scroll_top_request_counter = self.scroll_top_request_counter.wrapping_add(1);
        }
        true
    }

    /// A window fetch failed. Ignored if it is no longer the newest.
    pub fn fail_fetch(&mut self, gen: u64, msg: impl Into<String>) {
        if gen == self.fetch_gen {
            self.loading = false;
            self.set_flash_err(msg);
        }
    }

    /// Record what the mount-time backfill was fetched with. Separate from
    /// `finish_fetch` because a backfill *merges* into live records rather
    /// than replacing them.
    pub fn note_backfill(
        &mut self,
        filter: impl Into<ServerFilter>,
        limit: usize,
        returned: usize,
    ) {
        self.loaded = Loaded {
            filter: filter.into(),
            limit,
            truncated: limit > 0 && returned >= limit,
        };
    }

    /// Stop following the tail. New records collect in `pending`.
    pub fn pause(&mut self) {
        self.live = false;
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
        // The buffer holds what the server matched for `loaded.filter`. A
        // live record that does not match would never have been in that
        // result, so letting it in makes "N loaded for …" untrue within a
        // second of the search finishing — and fills the buffer with levels
        // whose buttons are off, pushing out the records that are wanted.
        if !self.loaded.filter.matches(&r) {
            return;
        }
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
        self.records
            .iter()
            .filter(|e| self.shows(&e.rec))
            .cloned()
            .collect()
    }

    /// Does the current view (level pills, query filter, app/host sets)
    /// include this record?
    #[must_use]
    pub fn shows(&self, r: &Record) -> bool {
        self.level_set.allows(r.level)
            && self.filter.matches(r)
            && self.app_set_allows(&r.app)
            && self.host_set_allows(&r.host)
    }

    /// How many buffered records the current view shows. Same predicate as
    /// `filtered`, without building the list.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.records.iter().filter(|e| self.shows(&e.rec)).count()
    }

    /// The visible row `delta` steps from `current` (negative = older).
    ///
    /// With nothing selected — or a selection the filter has since hidden —
    /// either direction lands on the newest visible row, which is where the
    /// eye already is in a tail. Clamps at both ends.
    #[must_use]
    pub fn neighbor_seq(&self, current: Option<u64>, delta: isize) -> Option<u64> {
        let visible: Vec<u64> = self
            .records
            .iter()
            .filter(|e| self.shows(&e.rec))
            .map(|e| e.seq)
            .collect();
        let last = visible.len().checked_sub(1)?;
        let Some(pos) = current.and_then(|c| visible.iter().position(|s| *s == c)) else {
            return visible.last().copied();
        };
        let target = pos.saturating_add_signed(delta).min(last);
        Some(visible[target])
    }

    /// Average records per minute across what is actually loaded.
    ///
    /// When the buffer was truncated it covers only the newest slice of the
    /// window, so dividing by the whole window (the old behaviour) reported
    /// a rate far below the real one. The span is then measured from the
    /// oldest loaded record instead.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn rate_per_min(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> u32 {
        let (mut n, mut oldest) = (0usize, to);
        for e in &self.records {
            let ts = e.rec.timestamp;
            if ts >= from && ts < to {
                n += 1;
                oldest = oldest.min(ts);
            }
        }
        if n == 0 {
            return 0;
        }
        let span_from = if self.loaded.truncated { oldest } else { from };
        let mins = ((to - span_from).num_milliseconds() as f64 / 60_000.0).max(1.0);
        (n as f64 / mins).round() as u32
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
        s.set_query("hello");
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
        s.app_set.insert("example_app".to_string());
        assert!(s.app_set_allows("example_app"));
    }

    #[test]
    fn app_set_allows_returns_false_when_app_not_in_set() {
        let mut s = AppState::new();
        s.app_set.insert("example_app".to_string());
        assert!(!s.app_set_allows("example_admin"));
    }

    #[test]
    fn toggle_app_inserts_then_removes() {
        let mut s = AppState::new();
        assert!(s.app_set.is_empty());
        s.toggle_app("example_app");
        assert!(s.app_set.contains("example_app"));
        s.toggle_app("example_app");
        assert!(!s.app_set.contains("example_app"));
    }

    #[test]
    fn toggle_host_inserts_then_removes() {
        let mut s = AppState::new();
        s.toggle_host("build-host");
        assert!(s.host_set.contains("build-host"));
        s.toggle_host("build-host");
        assert!(!s.host_set.contains("build-host"));
    }

    #[test]
    fn set_apps_replaces_existing_set() {
        let mut s = AppState::new();
        s.toggle_app("old");
        assert!(s.app_set.contains("old"));
        s.set_apps(["example_app".to_string(), "example_admin".to_string()]);
        assert!(!s.app_set.contains("old"));
        assert!(s.app_set.contains("example_app"));
        assert!(s.app_set.contains("example_admin"));
        assert_eq!(s.app_set.len(), 2);
    }

    #[test]
    fn set_hosts_replaces_existing_set() {
        let mut s = AppState::new();
        s.toggle_host("old");
        s.set_hosts(["build-host".to_string()]);
        assert!(!s.host_set.contains("old"));
        assert!(s.host_set.contains("build-host"));
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
        r1.app = "example_app".into();
        r1.host = "build-host".into();
        let mut r2 = rec("hello example_admin", Level::Info);
        r2.timestamp = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 1).unwrap();
        r2.app = "example_admin".into();
        r2.host = "prod-01".into();
        s.records.push_back(Entry::new(1, r1));
        s.records.push_back(Entry::new(2, r2));

        // Default state: no filter → both visible.
        assert_eq!(s.filtered().len(), 2);

        // app_set = {example_app} → only r1.
        s.toggle_app("example_app");
        let f = s.filtered();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rec.app, "example_app");

        // Add host filter that excludes the only remaining record.
        s.toggle_host("prod-01");
        assert_eq!(s.filtered().len(), 0);
    }

    fn rec_full(app: &str, lvl: Level, body: &str, secs: i64) -> Record {
        Record {
            timestamp: Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap()
                + chrono::Duration::seconds(secs),
            host: "h".into(),
            app: app.into(),
            stream_id: "s".into(),
            level: lvl,
            body: body.into(),
            labels: BTreeMap::new(),
            fields: BTreeMap::new(),
            raw: None,
        }
    }

    #[test]
    fn set_query_drives_the_structured_filter() {
        let mut s = AppState::new();
        s.push_record(rec_full("api", Level::Error, "db timeout", 0));
        s.push_record(rec_full("web", Level::Error, "db timeout", 1));
        s.push_record(rec_full("api", Level::Info, "db timeout", 2));
        s.set_query("level:error app:api timeout");
        assert_eq!(s.query(), "level:error app:api timeout");
        assert_eq!(s.visible_count(), 1);
        assert_eq!(s.filtered().len(), 1);
    }

    #[test]
    fn visible_count_agrees_with_filtered() {
        let mut s = AppState::new();
        for i in 0..20 {
            let lvl = if i % 3 == 0 {
                Level::Debug
            } else {
                Level::Info
            };
            s.push_record(rec_full("api", lvl, "x", i));
        }
        assert_eq!(s.visible_count(), s.filtered().len());
        assert!(
            s.visible_count() < s.records.len(),
            "debug is off by default"
        );
    }

    #[test]
    fn needs_server_search_is_false_until_something_is_loaded() {
        let mut s = AppState::new();
        assert!(!s.needs_server_search());
        s.set_query("timeout");
        assert!(!s.needs_server_search(), "no buffer yet to be stale");
    }

    #[test]
    fn needs_server_search_tracks_the_whole_request() {
        let mut s = AppState::new();
        let gen = s.begin_fetch();
        s.finish_fetch(gen, s.server_filter(), 100, vec![], FetchAnchor::Newest);
        assert!(!s.needs_server_search());

        // Free text.
        s.set_query("timeout");
        assert!(s.needs_server_search());
        let gen = s.begin_fetch();
        s.finish_fetch(gen, s.server_filter(), 100, vec![], FetchAnchor::Newest);
        assert!(!s.needs_server_search());

        // A structured token: the server filters on it too, so a buffer
        // loaded without it is no longer the right buffer.
        s.set_query("timeout app:billing");
        assert!(s.needs_server_search());
        let gen = s.begin_fetch();
        s.finish_fetch(gen, s.server_filter(), 100, vec![], FetchAnchor::Newest);

        // A level button.
        s.toggle_level(Level::Debug);
        assert!(s.needs_server_search());

        // And back: the same request as was loaded.
        s.toggle_level(Level::Debug);
        assert!(!s.needs_server_search());
    }

    #[test]
    fn server_filter_reflects_the_level_buttons() {
        let mut s = AppState::new();
        assert_eq!(
            s.server_filter().level_param().as_deref(),
            Some("info,warn,error,fatal"),
            "debug and trace are off by default"
        );
        s.toggle_level(Level::Debug);
        s.toggle_level(Level::Trace);
        assert_eq!(s.server_filter().levels, None, "everything on = any");
    }

    #[test]
    fn live_records_of_a_disabled_level_are_not_buffered() {
        let mut s = AppState::new();
        let gen = s.begin_fetch();
        s.finish_fetch(gen, s.server_filter(), 100, vec![], FetchAnchor::Newest);
        s.push_record(rec_full("api", Level::Debug, "noise", 0));
        s.push_record(rec_full("api", Level::Info, "signal", 1));
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.records[0].rec.body, "signal");
    }

    #[test]
    fn a_stale_fetch_response_is_discarded() {
        let mut s = AppState::new();
        let first = s.begin_fetch();
        let second = s.begin_fetch();
        assert!(!s.finish_fetch(
            first,
            "old",
            10,
            vec![rec_full("api", Level::Info, "old", 0)],
            FetchAnchor::Newest
        ));
        assert!(s.records.is_empty());
        assert!(s.loading, "the newer request is still in flight");
        assert!(s.finish_fetch(
            second,
            "new",
            10,
            vec![rec_full("api", Level::Info, "new", 0)],
            FetchAnchor::Newest
        ));
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.loaded.filter.text, "new");
        assert!(!s.loading);
    }

    #[test]
    fn a_stale_fetch_failure_does_not_clear_loading_or_flash() {
        let mut s = AppState::new();
        let first = s.begin_fetch();
        let _second = s.begin_fetch();
        s.fail_fetch(first, "boom");
        assert!(s.loading);
        assert!(s.flash.is_none());
    }

    #[test]
    fn truncated_is_set_when_the_server_fills_the_limit() {
        let mut s = AppState::new();
        let gen = s.begin_fetch();
        let rs: Vec<Record> = (0..5)
            .map(|i| rec_full("api", Level::Info, "x", i))
            .collect();
        s.finish_fetch(gen, String::new(), 5, rs, FetchAnchor::Newest);
        assert!(s.loaded.truncated);

        let gen = s.begin_fetch();
        let rs: Vec<Record> = (0..4)
            .map(|i| rec_full("api", Level::Info, "x", i))
            .collect();
        s.finish_fetch(gen, String::new(), 5, rs, FetchAnchor::Newest);
        assert!(!s.loaded.truncated);
    }

    #[test]
    fn note_backfill_records_truncation_without_touching_records() {
        let mut s = AppState::new();
        s.push_record(rec_full("api", Level::Info, "live", 0));
        s.note_backfill(String::new(), 500, 500);
        assert!(s.loaded.truncated);
        assert_eq!(s.loaded.limit, 500);
        assert_eq!(s.records.len(), 1);
    }

    #[test]
    fn live_records_outside_the_loaded_search_are_not_buffered() {
        let mut s = AppState::new();
        let gen = s.begin_fetch();
        s.finish_fetch(gen, "timeout", 100, vec![], FetchAnchor::Newest);
        s.push_record(rec_full("api", Level::Info, "all good", 0));
        s.push_record(rec_full("api", Level::Error, "db timeout", 1));
        assert_eq!(s.records.len(), 1);
        assert_eq!(s.records[0].rec.body, "db timeout");

        // Paused: the same rule applies to what is held back.
        s.pause();
        s.push_record(rec_full("api", Level::Info, "all good", 2));
        s.push_record(rec_full("api", Level::Error, "another timeout", 3));
        assert_eq!(s.pending_count, 1);
    }

    #[test]
    fn a_fetch_anchored_on_newest_asks_for_the_bottom() {
        let mut s = AppState::new();
        let (bottom, top) = (s.scroll_request_counter, s.scroll_top_request_counter);
        let gen = s.begin_fetch();
        let rs = vec![rec_full("api", Level::Info, "x", 0)];
        s.finish_fetch(gen, String::new(), 10, rs, FetchAnchor::Newest);
        assert_ne!(s.scroll_request_counter, bottom);
        assert_eq!(s.scroll_top_request_counter, top);
        assert!(s.live);
    }

    #[test]
    fn a_fetch_anchored_on_oldest_pauses_and_asks_for_the_top() {
        let mut s = AppState::new();
        let (bottom, top) = (s.scroll_request_counter, s.scroll_top_request_counter);
        let gen = s.begin_fetch();
        let rs = vec![rec_full("api", Level::Info, "x", 0)];
        s.finish_fetch(gen, String::new(), 10, rs, FetchAnchor::Oldest);
        assert_eq!(
            s.scroll_request_counter, bottom,
            "must not jump to the tail"
        );
        assert_ne!(s.scroll_top_request_counter, top);
        assert!(!s.live, "reading history: hold new records back");
    }

    #[test]
    fn pause_sends_new_records_to_pending() {
        let mut s = AppState::new();
        s.pause();
        s.push_record(rec_full("api", Level::Info, "x", 0));
        assert!(s.records.is_empty());
        assert_eq!(s.pending_count, 1);
    }

    #[test]
    fn neighbor_seq_steps_through_visible_rows_and_clamps() {
        let mut s = AppState::new();
        // seqs 0..5; odd ones are debug and hidden by the default level set.
        for i in 0..6 {
            let lvl = if i % 2 == 1 {
                Level::Debug
            } else {
                Level::Info
            };
            s.push_record(rec_full("api", lvl, "x", i));
        }
        // Nothing selected: land on the newest visible row either way.
        assert_eq!(s.neighbor_seq(None, 1), Some(4));
        assert_eq!(s.neighbor_seq(None, -1), Some(4));
        // Steps skip the hidden rows.
        assert_eq!(s.neighbor_seq(Some(4), -1), Some(2));
        assert_eq!(s.neighbor_seq(Some(2), -1), Some(0));
        assert_eq!(s.neighbor_seq(Some(0), 1), Some(2));
        // Clamped at both ends.
        assert_eq!(s.neighbor_seq(Some(0), -1), Some(0));
        assert_eq!(s.neighbor_seq(Some(4), 1), Some(4));
        // A selection the filter hides behaves like no selection.
        assert_eq!(s.neighbor_seq(Some(1), -1), Some(4));
    }

    #[test]
    fn neighbor_seq_is_none_when_nothing_is_visible() {
        let s = AppState::new();
        assert_eq!(s.neighbor_seq(None, 1), None);
    }

    #[test]
    fn rate_uses_the_whole_window_when_nothing_was_cut_off() {
        let mut s = AppState::new();
        let from = Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap();
        let to = from + chrono::Duration::minutes(10);
        for i in 0..100 {
            s.push_record(rec_full("api", Level::Info, "x", 540 + i % 60));
        }
        assert_eq!(s.rate_per_min(from, to), 10);
    }

    #[test]
    fn rate_measures_the_loaded_span_when_truncated() {
        let mut s = AppState::new();
        let from = Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap();
        let to = from + chrono::Duration::minutes(10);
        // 120 records, all inside the final two minutes of a ten-minute window.
        for i in 0..120 {
            s.push_record(rec_full("api", Level::Info, "x", 480 + i));
        }
        s.note_backfill(String::new(), 120, 120);
        assert_eq!(s.rate_per_min(from, to), 60);
    }

    #[test]
    fn rate_is_zero_for_an_empty_window() {
        let s = AppState::new();
        let from = Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap();
        assert_eq!(s.rate_per_min(from, from + chrono::Duration::minutes(5)), 0);
    }

    #[test]
    fn selection_select_keeps_the_detail_state() {
        let mut sel = Selection::default();
        sel.select(3);
        assert!(sel.is_selected(3) && !sel.is_expanded(3));
        sel.open(3);
        sel.select(4);
        assert!(sel.is_expanded(4), "an open pane follows the selection");
    }

    #[test]
    fn time_window_labels() {
        let p = |secs| TimeWindow::Preset(Duration::from_secs(secs)).label();
        assert_eq!(p(900), "Last 15 min");
        assert_eq!(p(21_600), "Last 6 h");
        assert_eq!(p(7200), "Last 2 h");
        assert_eq!(p(120), "Last 2 min");
        assert_eq!(p(45), "Last 45 s");
        let from = Utc.with_ymd_and_hms(2026, 4, 24, 12, 0, 0).unwrap();
        let w = TimeWindow::Custom {
            from,
            to: from + chrono::Duration::hours(1),
        };
        assert_eq!(w.label(), "04-24 12:00 → 04-24 13:00 UTC");
    }
}
