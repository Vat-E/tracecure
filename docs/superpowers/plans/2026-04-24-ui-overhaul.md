# UI Overhaul Implementation Plan — unified stream, Leptos 0.8, Mezmo-principled

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current two-route (`/tail`, `/search`) Leptos 0.6 UI with a single unified log stream on Leptos 0.8 — time-window-driven, hybrid live mode (scroll-up auto-pauses, explicit resume required), dense monospace rows, `:` command palette, and the same keybindings as the TUI.

**Architecture:** Port pure-Rust modules from `sift-cli/src/tui/*` (jsonfmt, command parser, state, keys) into `ui/sift-ui/src/*` first — these don't depend on Leptos and are testable with `cargo test`. Then bump Leptos 0.6 → 0.8 (big API drift), rebuild all components from scratch against the new API and the v6 mockup (`.superpowers/brainstorm/*/content/sift-v6.html`). Old `routes/tail.rs`, `routes/search.rs`, and all 10 existing components get deleted in the cutover.

**Tech Stack:** Rust 2021 · Leptos 0.8 · leptos_router 0.8 · gloo-net 0.6 · gloo-timers 0.3 · web-sys 0.3 · wasm-bindgen 0.2 · serde + serde_json.

**Spec:** `docs/superpowers/specs/2026-04-24-ui-overhaul-design.md` (commit `425b64b`).

---

## Ground rules for every task

- **One concern per commit.** Each task ends with a single commit.
- **TDD for pure modules.** Tasks T1–T4 (`keys`, `jsonfmt`, `command`, `state`) are pure Rust with native unit tests (`cargo test -p sift-ui`).
- **Compile-check gate for Leptos components.** Component tasks (T8–T17) verify with `cargo build -p sift-ui --target wasm32-unknown-unknown` AND a `trunk build` in T25's smoke step.
- **Always run `cargo fmt --all`** after editing Rust files.
- **Always run `cargo clippy -p sift-ui --target wasm32-unknown-unknown --all-targets -- -D warnings -W clippy::pedantic`** before committing component tasks. Pure-module tasks use host-target clippy (`cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic`) since tests run natively.
- **`cargo test -p sift-ui`** must stay green throughout. After T1–T4: new tests accumulate. After T5 (Leptos bump): must still pass. After T7–T24: must still pass.
- **Dep-freshness rule:** if a newer stable Leptos (or any other dep) has dropped, use it. Run `cargo update` after every `Cargo.toml` edit.
- **Never skip git hooks.** Fix hook failures with a new commit.
- **`trunk build --release` must succeed** in the final verification (after T25).

---

## File structure (target)

```
ui/sift-ui/
├─ Cargo.toml              # MODIFIED: Leptos 0.6 → 0.8, fresh deps
├─ index.html              # UNCHANGED (trunk entry)
├─ Trunk.toml              # UNCHANGED
├─ styles.css              # NEW (previously none) — CSS vars, base layout
└─ src/
    ├─ main.rs             # UNCHANGED in shape (Leptos mount), minor API updates
    ├─ lib.rs              # NEW if needed (module registration)
    ├─ app.rs              # REWRITTEN — single `/` route, AppState signal, shell
    ├─ state.rs            # NEW — AppState struct + methods + tests
    ├─ keys.rs             # NEW — KEYBIND_TABLE const + tests
    ├─ jsonfmt.rs          # NEW (replaces components/json_highlight.rs) — JSON tokenizer + tests
    ├─ command.rs          # NEW — `:` palette parser + tests
    ├─ ws.rs               # REWRITTEN — adapt to AppState, keep backoff logic
    ├─ query.rs            # NEW — `/v1/search` fetch helper (async)
    └─ components/
        ├─ mod.rs          # REWRITTEN — re-export new components only
        ├─ rail.rs         # NEW — left rail (brand + icons)
        ├─ top_bar.rs      # NEW — range + level pills + line count + LIVE
        ├─ stream.rs       # NEW — scrollable stream container
        ├─ row.rs          # NEW — single log row
        ├─ row_detail.rs   # NEW — inline expand detail
        ├─ paused_banner.rs # NEW — floating resume pill
        ├─ search_row.rs   # NEW — bottom query input + :commands button
        ├─ status_bar.rs   # NEW — bottom status strip
        ├─ help_modal.rs   # NEW (replaces old) — `?` overlay using KEYBIND_TABLE
        └─ command_modal.rs # NEW — `:` palette modal

DELETED at cutover (T7):
├─ src/routes/mod.rs
├─ src/routes/tail.rs
├─ src/routes/search.rs
├─ src/components/filter_bar.rs
├─ src/components/help_modal.rs      (OLD — new one replaces)
├─ src/components/json_highlight.rs
├─ src/components/level_toggle.rs
├─ src/components/pause_button.rs
├─ src/components/record_detail.rs
├─ src/components/record_row.rs
├─ src/components/status_pill.rs
└─ src/components/time_range.rs
```

---

## Task 1: `keys.rs` — KEYBIND_TABLE source of truth

**Goal:** One constant defining all UI keybindings. Mirrors the TUI's `view_help.rs::KEYBIND_TABLE`. Both the help modal and the keyboard dispatcher read from this.

**Files:**
- Create: `ui/sift-ui/src/keys.rs`
- Modify: `ui/sift-ui/src/main.rs` (register `mod keys;`)

- [ ] **Step 1: Create `keys.rs`**

```rust
//! Single source of truth for UI keybindings. Help modal and keyboard
//! dispatcher both read from this table.

pub const KEYBIND_TABLE: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("Ctrl+C", "quit / blur the page"),
            ("?", "toggle help"),
            ("Esc", "close overlay / collapse detail"),
            (":", "open command palette"),
            ("/", "focus query input"),
            ("Ctrl+T", "toggle absolute / relative timestamps"),
        ],
    ),
    (
        "Stream",
        &[
            ("j / ↓", "next row"),
            ("k / ↑", "previous row"),
            ("g / Home", "top of window"),
            ("G / End", "jump to latest + resume live"),
            ("Enter", "toggle detail for selected row"),
            ("Space", "pause / resume"),
            ("r", "force reconnect"),
            ("I W E F", "toggle Info / Warn / Error / Fatal"),
            ("D T", "toggle Debug / Trace"),
        ],
    ),
    (
        "Command palette",
        &[
            (":pause / :resume / :clear / :retry", ""),
            (":save path=<file>", "download buffer as NDJSON"),
            (":search q=<str> from=<iso> to=<iso>", ""),
            (":level <name> on|off", ""),
            (":help / :quit", ""),
            ("Tab", "complete verb prefix"),
            ("Esc", "cancel"),
        ],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_three_sections() {
        let titles: Vec<&str> = KEYBIND_TABLE.iter().map(|(t, _)| *t).collect();
        assert_eq!(titles, vec!["Global", "Stream", "Command palette"]);
    }

    #[test]
    fn every_section_has_at_least_one_entry() {
        for (title, entries) in KEYBIND_TABLE {
            assert!(!entries.is_empty(), "section '{title}' is empty");
        }
    }

    #[test]
    fn global_has_ctrl_c_and_colon_and_slash() {
        let (_, global) = KEYBIND_TABLE
            .iter()
            .find(|(t, _)| *t == "Global")
            .expect("Global section exists");
        let keys: Vec<&str> = global.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Ctrl+C"));
        assert!(keys.contains(&":"));
        assert!(keys.contains(&"/"));
    }
}
```

- [ ] **Step 2: Register module**

Edit `ui/sift-ui/src/main.rs`. Near the other `mod` declarations, add:

```rust
mod keys;
```

(If `main.rs` has no module declarations today, just add `mod keys;` at the top under any `use` lines.)

- [ ] **Step 3: Verify tests pass**

Run from repo root:
```bash
cargo test -p sift-ui --lib keys
```
Expected: 3 tests pass. (The `--lib` flag picks up lib/bin unit tests; works whether sift-ui is a bin or lib crate.)

- [ ] **Step 4: Format + clippy**

```bash
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
```
Expected: clean. Note: `-D clippy::pedantic` may flag `ui-sift` pedantically — add `#[allow(clippy::items_after_statements)]` or similar if false positives emerge. If clippy complains `KEYBIND_TABLE` is never used (likely — nothing else references it yet), add `#[allow(dead_code)]` above the const.

- [ ] **Step 5: Commit**

```bash
git add ui/sift-ui/src/keys.rs ui/sift-ui/src/main.rs
git commit -m "feat(ui): keys.rs — KEYBIND_TABLE source of truth for help + dispatcher"
```

---

## Task 2: `jsonfmt.rs` — JSON tokenizer (port from sift-cli)

**Goal:** Pure-Rust JSON tokenizer that produces a `Vec<JsonToken>` for Leptos components to render as styled spans. Replaces `components/json_highlight.rs` (219 lines) which does similar work but is tangled with Leptos.

**Files:**
- Create: `ui/sift-ui/src/jsonfmt.rs`
- Modify: `ui/sift-ui/src/main.rs` (register module)

- [ ] **Step 1: Study reference**

Read `crates/sift-cli/src/tui/jsonfmt.rs` to understand the recursive walker + DEPTH_LIMIT + color-per-type structure. Port the structure, not the ratatui types.

- [ ] **Step 2: Create `jsonfmt.rs`**

```rust
//! Pure JSON tokenizer. Takes a `serde_json::Value`, produces a flat
//! `Vec<JsonToken>` where each token carries its semantic color.
//!
//! Consumers (Leptos `row_detail` component) render each token as a
//! styled `<span>` — color CSS classes map to theme variables.
//!
//! Bounded by DEPTH_LIMIT to avoid pathological nesting.

use serde_json::Value;

pub const DEPTH_LIMIT: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonColor {
    Key,        // object keys
    Str,        // string values
    Num,        // number values
    Bool,       // true/false
    Null,       // null
    Punct,      // { } [ ] : , and whitespace
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonToken {
    pub color: JsonColor,
    pub text: String,
}

/// Tokenize a JSON value into colored tokens. Pretty-printed output
/// (2-space indent, newlines between object/array members).
#[must_use]
pub fn tokenize(v: &Value) -> Vec<JsonToken> {
    let mut out = Vec::new();
    walk(v, &mut out, 0);
    out
}

fn walk(v: &Value, out: &mut Vec<JsonToken>, depth: usize) {
    if depth > DEPTH_LIMIT {
        out.push(JsonToken { color: JsonColor::Punct, text: "…".to_string() });
        return;
    }
    match v {
        Value::Null => out.push(JsonToken { color: JsonColor::Null, text: "null".to_string() }),
        Value::Bool(b) => out.push(JsonToken { color: JsonColor::Bool, text: b.to_string() }),
        Value::Number(n) => out.push(JsonToken { color: JsonColor::Num, text: n.to_string() }),
        Value::String(s) => {
            out.push(JsonToken { color: JsonColor::Str, text: format!("\"{}\"", escape_json_string(s)) });
        }
        Value::Array(arr) => {
            out.push(JsonToken { color: JsonColor::Punct, text: "[".to_string() });
            for (i, item) in arr.iter().enumerate() {
                if i > 0 {
                    out.push(JsonToken { color: JsonColor::Punct, text: ", ".to_string() });
                }
                walk(item, out, depth + 1);
            }
            out.push(JsonToken { color: JsonColor::Punct, text: "]".to_string() });
        }
        Value::Object(map) => {
            out.push(JsonToken { color: JsonColor::Punct, text: "{\n".to_string() });
            let indent = "  ".repeat(depth + 1);
            for (i, (k, val)) in map.iter().enumerate() {
                out.push(JsonToken { color: JsonColor::Punct, text: indent.clone() });
                out.push(JsonToken { color: JsonColor::Key, text: format!("\"{}\"", escape_json_string(k)) });
                out.push(JsonToken { color: JsonColor::Punct, text: ": ".to_string() });
                walk(val, out, depth + 1);
                if i + 1 < map.len() {
                    out.push(JsonToken { color: JsonColor::Punct, text: ",".to_string() });
                }
                out.push(JsonToken { color: JsonColor::Punct, text: "\n".to_string() });
            }
            out.push(JsonToken { color: JsonColor::Punct, text: "  ".repeat(depth) + "}" });
        }
    }
}

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors(tokens: &[JsonToken]) -> Vec<JsonColor> {
        tokens.iter().map(|t| t.color).collect()
    }

    #[test]
    fn primitive_null() {
        let t = tokenize(&Value::Null);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].color, JsonColor::Null);
        assert_eq!(t[0].text, "null");
    }

    #[test]
    fn primitive_bool() {
        assert_eq!(tokenize(&Value::Bool(true))[0].text, "true");
        assert_eq!(tokenize(&Value::Bool(false))[0].text, "false");
    }

    #[test]
    fn primitive_number() {
        let t = tokenize(&serde_json::json!(42));
        assert_eq!(t[0].color, JsonColor::Num);
        assert_eq!(t[0].text, "42");
    }

    #[test]
    fn primitive_string_quotes_are_included() {
        let t = tokenize(&serde_json::json!("hello"));
        assert_eq!(t[0].color, JsonColor::Str);
        assert_eq!(t[0].text, "\"hello\"");
    }

    #[test]
    fn object_has_key_then_value() {
        let v = serde_json::json!({"n": 1});
        let t = tokenize(&v);
        // Expect: Punct("{\n"), Punct("  "), Key("\"n\""), Punct(": "), Num("1"), Punct("\n"), Punct("}")
        // Just verify the key-value pair is present and colored correctly.
        let key_idx = t.iter().position(|t| t.color == JsonColor::Key).expect("has a key");
        assert_eq!(t[key_idx].text, "\"n\"");
        let num_idx = t.iter().position(|t| t.color == JsonColor::Num).expect("has a number");
        assert_eq!(t[num_idx].text, "1");
    }

    #[test]
    fn array_separates_with_commas() {
        let v = serde_json::json!([1, 2, 3]);
        let t = tokenize(&v);
        let nums: Vec<&str> = t.iter()
            .filter(|t| t.color == JsonColor::Num)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(nums, vec!["1", "2", "3"]);
    }

    #[test]
    fn string_escapes_are_applied() {
        let v = serde_json::json!("line1\nline2");
        let t = tokenize(&v);
        assert_eq!(t[0].text, "\"line1\\nline2\"");
    }

    #[test]
    fn depth_limit_caps_recursion() {
        // Build a deeply nested structure
        let mut v = Value::Null;
        for _ in 0..(DEPTH_LIMIT + 10) {
            v = Value::Array(vec![v]);
        }
        let t = tokenize(&v);
        // Should find the ellipsis marker somewhere
        assert!(t.iter().any(|t| t.text == "…"));
    }
}
```

- [ ] **Step 3: Register module**

In `ui/sift-ui/src/main.rs`:

```rust
mod jsonfmt;
```

(Next to `mod keys;`.)

- [ ] **Step 4: Verify tests pass**

```bash
cargo test -p sift-ui --lib jsonfmt
```
Expected: 8 tests pass.

- [ ] **Step 5: Format + clippy**

```bash
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
```
Expected: clean. Add `#[allow(dead_code)]` on the public items if clippy flags them as unused (nothing consumes `JsonToken` yet; T12 will).

- [ ] **Step 6: Commit**

```bash
git add ui/sift-ui/src/jsonfmt.rs ui/sift-ui/src/main.rs
git commit -m "feat(ui): jsonfmt.rs — pure JSON tokenizer (port from sift-cli, Leptos-free)"
```

---

## Task 3: `command.rs` — `:` palette parser (port from sift-cli)

**Goal:** The TUI's `:` palette grammar becomes the UI's palette grammar. Literally copy the module with its 9 verbs (`:pause`, `:resume`, `:clear`, `:retry`, `:save`, `:search`, `:level`, `:help`, `:quit`) and its `UnterminatedQuote` guard. Pure Rust, no Leptos.

**Files:**
- Create: `ui/sift-ui/src/command.rs`
- Modify: `ui/sift-ui/src/main.rs`

- [ ] **Step 1: Copy the source from sift-cli**

Read `crates/sift-cli/src/tui/command.rs` in full. Copy its contents verbatim to `ui/sift-ui/src/command.rs` as the starting point. Adjust ONLY these:

- Remove the `#![allow(dead_code)]` module-level attribute (if present) — we'll add per-item allows only if clippy demands them.
- Keep all 9 `CommandAction` variants, all `CommandError` variants including `UnterminatedQuote(String)`, `parse()`, `complete()`, `tokenize()`, `Completion`, helper fns, and all 19 tests.

- [ ] **Step 2: Register module**

In `ui/sift-ui/src/main.rs`:

```rust
mod command;
```

(Next to `mod keys;`, `mod jsonfmt;`.)

- [ ] **Step 3: Verify tests pass**

```bash
cargo test -p sift-ui --lib command
```
Expected: 19 tests pass. (Same count as sift-cli's `tui::command::tests`.)

- [ ] **Step 4: Format + clippy**

```bash
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
```
Expected: clean. Since the port is verbatim from a clippy-clean source, should require no new allows. If any arise from crate-local context, add minimally.

- [ ] **Step 5: Commit**

```bash
git add ui/sift-ui/src/command.rs ui/sift-ui/src/main.rs
git commit -m "feat(ui): command.rs — :palette parser ported verbatim from sift-cli"
```

---

## Task 4: `state.rs` — AppState + methods

**Goal:** Plain-Rust `AppState` struct with pure methods. No Leptos — the shell in T7 wraps it in a signal. Testable natively.

**Files:**
- Create: `ui/sift-ui/src/state.rs`
- Modify: `ui/sift-ui/src/main.rs`

- [ ] **Step 1: Create `state.rs`**

```rust
//! AppState — plain Rust, no Leptos. The shell component wraps an
//! `AppState` in an `RwSignal` and every component that reads/writes
//! state goes through that.
//!
//! Methods mirror the TUI's `AppState` (crates/sift-cli/src/tui/state.rs)
//! wherever semantics match, so the mental model carries over.

use std::collections::VecDeque;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sift_core::{Level, Record};

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
    Custom { from: DateTime<Utc>, to: DateTime<Utc> },
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
        Self { info: true, warn: true, error: true, fatal: true, debug: false, trace: false }
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
pub enum FlashKind { Ok, Err }

#[derive(Debug, Clone)]
pub struct Flash {
    pub msg: String,
    pub kind: FlashKind,
    pub expires_at_ms: u64, // epoch millis; client clears when now > this
}

#[derive(Debug)]
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
    fn default() -> Self { Self::new() }
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

    /// Filtered view based on level_set + query substring.
    #[must_use]
    pub fn filtered(&self) -> Vec<&Record> {
        let q = self.query.as_str();
        self.records
            .iter()
            .filter(|r| self.level_set.allows(r.level))
            .filter(|r| q.is_empty() || r.body.contains(q))
            .collect()
    }

    pub fn toggle_level(&mut self, lvl: Level) { self.level_set.toggle(lvl); }

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

    pub fn toggle_help(&mut self) { self.help_open = !self.help_open; }
}

#[cfg(target_arch = "wasm32")]
fn now_ms() -> u64 {
    (js_sys::Date::now() as u64)
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use sift_core::{Level, Record};
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
```

- [ ] **Step 2: Register module**

In `ui/sift-ui/src/main.rs`:

```rust
mod state;
```

- [ ] **Step 3: Verify compile + tests**

```bash
cargo test -p sift-ui --lib state
```
Expected: 11 tests pass.

If `sift_core::Record` fields differ from what's expected (`stream_id`, `labels` as BTreeMap, etc.), adapt the test helper `rec()` accordingly. Grep `crates/sift-core/src/lib.rs` for the current `Record` definition.

- [ ] **Step 4: Format + clippy**

```bash
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
```
Expected: clean. May need `#[allow(dead_code)]` on methods not yet consumed.

- [ ] **Step 5: Commit**

```bash
git add ui/sift-ui/src/state.rs ui/sift-ui/src/main.rs
git commit -m "feat(ui): state.rs — AppState struct + methods (mirror of TUI)"
```

---

## Task 5: Leptos 0.6 → 0.8 bump + legacy cleanup

**Goal:** Upgrade Leptos to the latest stable (0.8.19 as of 2026-04-24, verify at task time), fix the API drift fallout, and DELETE the old routes and components in the same commit. After this task, `cargo build --target wasm32-unknown-unknown` and `trunk build` succeed on a bare skeleton; tasks 7+ rebuild the app.

**Files:**
- Modify: `ui/sift-ui/Cargo.toml`
- Modify: `ui/sift-ui/src/main.rs`, `ui/sift-ui/src/app.rs`, `ui/sift-ui/src/ws.rs`
- Delete: `ui/sift-ui/src/routes/` (entire directory)
- Delete: `ui/sift-ui/src/components/` (entire directory — we rebuild from scratch)

- [ ] **Step 1: Check latest Leptos stable**

```bash
cargo search leptos --limit 1
cargo search leptos_router --limit 1
```
Use whatever is latest. This plan assumes `0.8.x` — adapt version number in Cargo.toml accordingly.

- [ ] **Step 2: Update `ui/sift-ui/Cargo.toml`**

Replace the Leptos + leptos_router lines:

```toml
leptos = { version = "0.8", features = ["csr"] }
leptos_router = { version = "0.8", features = ["csr"] }
```

Check other deps for freshness too (`wasm-bindgen`, `web-sys`, `gloo-net`, `gloo-timers`). Bump any that are >1 minor behind per the CLAUDE.md dep-freshness rule.

- [ ] **Step 3: `cargo update`**

```bash
cargo update
```

- [ ] **Step 4: Delete the legacy `routes/` directory**

```bash
rm -rf ui/sift-ui/src/routes
```

Windows bash note: `rm -rf` works with Git Bash / WSL-style path. If that fails, use PowerShell `Remove-Item -Recurse -Force ui/sift-ui/src/routes`.

- [ ] **Step 5: Delete the legacy `components/` directory**

```bash
rm -rf ui/sift-ui/src/components
```

We rebuild these entirely in T8–T17.

- [ ] **Step 6: Gut `app.rs` to a skeleton**

Replace the entire contents of `ui/sift-ui/src/app.rs` with:

```rust
//! App shell — minimal skeleton during migration. Task 7 fills this in
//! with the new router + AppState signal + component tree.

use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <div style="padding:20px;color:#ccc;background:#111;height:100vh;font-family:monospace">
            "Sift UI — under reconstruction (Leptos 0.8 migration)"
        </div>
    }
}
```

**Leptos 0.8 note:** imports are `use leptos::prelude::*;` (0.7+ consolidated the prelude). Use the correct `#[component]` attribute + `view!` macro syntax for 0.8. If the exact symbol is different in 0.8, consult <https://docs.rs/leptos/latest> — the version resolved in step 3 is the source of truth.

- [ ] **Step 7: Gut `ws.rs` to a stub**

Replace the entire contents of `ui/sift-ui/src/ws.rs` with:

```rust
//! WebSocket client — stub during migration. Task 18 fills this in
//! against the new AppState shape.

#[allow(dead_code)]
pub fn placeholder() { /* replaced in T18 */ }
```

- [ ] **Step 8: Update `main.rs`**

Make sure `main.rs` only references modules that exist now:

```rust
mod app;
mod command;
mod jsonfmt;
mod keys;
mod state;
mod ws;

use app::App;

fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Info).ok();
    leptos::mount::mount_to_body(App);
}
```

**Leptos 0.8 note:** the mount API is `leptos::mount::mount_to_body` (changed from 0.6's `leptos::mount_to_body`). Verify exact path.

- [ ] **Step 9: Build**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
```

Expected: clean build. If the `view!` macro or `mount_to_body` path is different, fix per Leptos 0.8 docs.

Also verify trunk:
```bash
cd ui/sift-ui && trunk build
```
Expected: builds an `index.html` + `.wasm` bundle. (No need for `--release`.)

- [ ] **Step 10: Verify unit tests still pass**

```bash
cd ../..  # back to repo root
cargo test -p sift-ui
```
Expected: all pure-module tests (T1–T4: ~41 tests total) still pass. Leptos 0.6→0.8 doesn't touch those modules.

- [ ] **Step 11: Clippy**

```bash
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
```
Expected: clean.

- [ ] **Step 12: Commit**

```bash
git add -A ui/sift-ui
git commit -m "chore(ui): bump Leptos 0.6 → 0.8 + delete legacy routes/components"
```

Commit body (via HEREDOC):
```
Leptos 0.8.x brings a reworked reactive graph, renamed signal API,
and a consolidated prelude. Existing 0.6 components don't compile
against 0.8 without a full rewrite — T8–T17 rebuild them against
the new API + the v6 mockup design.

Legacy files removed:
- src/routes/ (tail.rs, search.rs, mod.rs)
- src/components/ (10 components — will be rebuilt as 10 different
  components matching the new design)

Pure modules unchanged: keys, jsonfmt, command, state (all pass tests).
App skeleton compiles; trunk build succeeds on an empty shell.
```

---

## Task 6: `styles.css` — CSS variables + base layout

**Goal:** One stylesheet with CSS variables, base reset, and class rules. Based on the v6 mockup palette and layout. Loaded by `index.html`.

**Files:**
- Create: `ui/sift-ui/styles.css`
- Modify: `ui/sift-ui/index.html` (add `<link rel="stylesheet" href="styles.css">`)

- [ ] **Step 1: Create `styles.css`**

Copy the entire `<style>` block from `.superpowers/brainstorm/948-1777043807/content/sift-v6.html` (everything between `<style>` and `</style>`). Strip the `<style>` tags themselves. Save as `ui/sift-ui/styles.css`.

Key sections the file must contain:
- `:root` CSS variables for colors (bg, bg-2, panel, panel-2, border, border-focus, text, text-muted, text-dim, accent, accent-bg, ok, warn, error, fatal, debug)
- Universal box-sizing + margin reset
- `.app` flex layout (rail + main)
- `.rail`, `.main`, `.top`, `.stream`, `.search-row`, `.status` layout
- `.row` grid layout with level-specific color classes
- `.resume` paused banner
- `.lvl-pill`, `.range`, `.live-btn`, `.lines` top bar pieces
- `.query`, `.cmd` search row pieces
- Scrollbar styling

- [ ] **Step 2: Wire styles.css into index.html**

Read `ui/sift-ui/index.html`. Add inside `<head>`:

```html
<link data-trunk rel="css" href="styles.css" />
```

(The `data-trunk` attribute tells Trunk to pick it up and hash-fingerprint it.)

- [ ] **Step 3: Verify trunk builds with styles**

```bash
cd ui/sift-ui && trunk build
ls dist/  # should include hashed styles.css
```

- [ ] **Step 4: Commit**

```bash
cd ../..
git add ui/sift-ui/styles.css ui/sift-ui/index.html
git commit -m "feat(ui): styles.css — palette + layout (from v6 mockup)"
```

---

## Task 7: `app.rs` — single route, AppState signal, shell

**Goal:** `App` component becomes the real shell. Creates the `AppState` signal, parses legacy URL params on mount, renders the 4 layout slots (rail, top bar, stream+paused-banner, search row, status bar).

**Files:**
- Modify: `ui/sift-ui/src/app.rs`
- Create: `ui/sift-ui/src/components/mod.rs`

- [ ] **Step 1: Create components module**

```bash
mkdir -p ui/sift-ui/src/components
```

Create `ui/sift-ui/src/components/mod.rs` with empty re-exports (filled in T8–T17):

```rust
//! New UI components (v2 design — see spec 2026-04-24-ui-overhaul-design.md).

pub mod rail;
pub mod top_bar;
pub mod stream;
pub mod row;
pub mod row_detail;
pub mod paused_banner;
pub mod search_row;
pub mod status_bar;
pub mod help_modal;
pub mod command_modal;
```

**Note:** this creates broken references to files that don't exist yet. The next sub-steps create empty stub files for each so the build is green after T7. Each of T8–T17 replaces one stub with the real component.

- [ ] **Step 2: Create empty stub files**

For each of rail, top_bar, stream, row, row_detail, paused_banner, search_row, status_bar, help_modal, command_modal — create `ui/sift-ui/src/components/<name>.rs` with:

```rust
//! Stub — filled in by task TN.

use leptos::prelude::*;

#[component]
pub fn PascalCaseName() -> impl IntoView {
    view! { <div></div> }
}
```

Replace `PascalCaseName` with the component name (e.g. `Rail`, `TopBar`, `Stream`, `Row`, `RowDetail`, `PausedBanner`, `SearchRow`, `StatusBar`, `HelpModal`, `CommandModal`). Ten stub files.

- [ ] **Step 3: Rewrite `app.rs`**

Replace `ui/sift-ui/src/app.rs` with:

```rust
//! App shell — owns the AppState signal, builds the layout.

use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::components::{
    command_modal::CommandModal,
    help_modal::HelpModal,
    paused_banner::PausedBanner,
    rail::Rail,
    search_row::SearchRow,
    status_bar::StatusBar,
    stream::Stream,
    top_bar::TopBar,
};
use crate::state::AppState;

/// Global state context — every component reads via `expect_context`.
#[derive(Clone, Copy)]
pub struct AppCtx(pub RwSignal<AppState>);

#[component]
pub fn App() -> impl IntoView {
    // Shared state signal.
    let state = RwSignal::new(AppState::new());
    provide_context(AppCtx(state));

    // Legacy URL migration (best-effort): if the loaded URL is
    // /tail or /search, parse params into state and rewrite to /.
    migrate_legacy_url(state);

    view! {
        <Router>
            <Routes fallback=|| view! { <Home/> }>
                <Route path=path!("/") view=Home/>
            </Routes>
        </Router>
    }
}

#[component]
fn Home() -> impl IntoView {
    view! {
        <div class="app">
            <Rail/>
            <main class="main">
                <TopBar/>
                <Stream/>
                <SearchRow/>
                <StatusBar/>
            </main>
            <PausedBanner/>
            <HelpModal/>
            <CommandModal/>
        </div>
    }
}

fn migrate_legacy_url(state: RwSignal<AppState>) {
    use web_sys::window;
    let Some(w) = window() else { return };
    let Ok(loc) = w.location().href() else { return };
    let Ok(url) = web_sys::Url::new(&loc) else { return };

    let path = url.pathname();
    if path == "/tail" || path == "/search" {
        // Extract ?q=... and populate state.query if present.
        let params = url.search_params();
        if let Some(q) = params.get("q") {
            state.update(|s| s.query = q);
        }
        // Replace URL to / without reloading.
        if let Ok(history) = w.history() {
            let _ = history.replace_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some("/"));
        }
    }
}
```

**Leptos 0.8 router note:** the router API moved to `leptos_router::components::*` and the `path!` macro. Verify exact symbol names against the 0.8 docs. If `Routes fallback=...` signature differs, adapt.

- [ ] **Step 4: Build check**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cd ui/sift-ui && trunk build && cd ../..
```
Expected: clean build, empty app renders.

- [ ] **Step 5: Format + clippy**

```bash
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
```

- [ ] **Step 6: Commit**

```bash
git add ui/sift-ui/src/app.rs ui/sift-ui/src/components/
git commit -m "feat(ui): app shell — single route, AppState signal, component stubs"
```

---

## Task 8: `components/rail.rs` — left rail

**Goal:** 40px left rail with brand + Logs icon (active) + Settings + Help.

**Files:** Modify `ui/sift-ui/src/components/rail.rs`.

- [ ] **Step 1: Replace `rail.rs` contents**

```rust
//! Left icon rail — 40px wide, full height. Brand + Logs (active)
//! + Settings + Help.

use leptos::prelude::*;

use crate::app::AppCtx;

#[component]
pub fn Rail() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    view! {
        <nav class="rail">
            <div class="brand">"s"</div>
            <div class="icon active" title="Logs">"☰"</div>
            <div class="spacer"></div>
            <div class="icon" title="Settings">"⚙"</div>
            <div class="icon" title="Help · ?"
                 on:click=move |_| ctx.0.update(|s| s.toggle_help())>
                "?"
            </div>
        </nav>
    }
}
```

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/rail.rs
git commit -m "feat(ui): rail component — brand + Logs + Settings + Help"
```

---

## Task 9: `components/top_bar.rs` — range + level pills + line count + LIVE

**Goal:** The top 44px row. Clicking a level pill toggles the corresponding `level_set` field. Clicking LIVE toggles live on/off. Line count shows `records.len()` formatted with commas.

**Files:** Modify `ui/sift-ui/src/components/top_bar.rs`.

- [ ] **Step 1: Replace `top_bar.rs` contents**

```rust
//! Top bar — time range picker, level pills, line count, LIVE button.

use leptos::prelude::*;
use sift_core::Level;

use crate::app::AppCtx;

#[component]
pub fn TopBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    // Signals derived from state for reactivity.
    let lines = move || state.with(|s| s.records.len());
    let live = move || state.with(|s| s.live);
    let level_set = move || state.with(|s| s.level_set);

    view! {
        <div class="top">
            <div class="range">
                <span class="ic">"◷"</span>
                <span>"Last 1 h"</span>
                <span class="chev">"▾"</span>
            </div>
            <div class="levels">
                {move || level_pill("I", Level::Info, level_set().info, "var(--ok)", state)}
                {move || level_pill("W", Level::Warn, level_set().warn, "var(--warn)", state)}
                {move || level_pill("E", Level::Error, level_set().error, "var(--error)", state)}
                {move || level_pill("F", Level::Fatal, level_set().fatal, "var(--fatal)", state)}
                {move || level_pill("D", Level::Debug, level_set().debug, "var(--debug)", state)}
                {move || level_pill("T", Level::Trace, level_set().trace, "var(--text-dim)", state)}
            </div>
            <div class="spacer"></div>
            <div class="lines">
                <span class="n">{move || format_commas(lines())}</span>
                <span class="u">"lines"</span>
            </div>
            <div class="live-btn"
                 on:click=move |_| state.update(|s| if s.live { s.live = false } else { s.resume_live() })>
                <span class="d"></span>
                {move || if live() { "LIVE" } else { "PAUSED" }}
            </div>
        </div>
    }
}

fn level_pill(
    letter: &'static str,
    lvl: Level,
    on: bool,
    color_var: &'static str,
    state: RwSignal<crate::state::AppState>,
) -> impl IntoView {
    let class = if on { "lvl-pill on" } else { "lvl-pill off" };
    let style = if on { format!("color:{color_var}") } else { String::new() };
    view! {
        <div class=class style=style
             on:click=move |_| state.update(|s| s.toggle_level(lvl))>
            <span class="d" style=format!("background:{color_var}")></span>
            {letter}
        </div>
    }
}

fn format_commas(n: usize) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        let from_end = bytes.len() - i;
        if i > 0 && from_end % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_commas_basic() {
        assert_eq!(format_commas(0), "0");
        assert_eq!(format_commas(999), "999");
        assert_eq!(format_commas(1_000), "1,000");
        assert_eq!(format_commas(12_438), "12,438");
        assert_eq!(format_commas(1_234_567), "1,234,567");
    }
}
```

- [ ] **Step 2: Verify tests pass**

```bash
cargo test -p sift-ui --lib components::top_bar
```
Expected: 1 test passes.

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/top_bar.rs
git commit -m "feat(ui): top_bar — time range + level pills + line count + LIVE toggle"
```

---

## Task 10: `components/stream.rs` — scrollable stream container

**Goal:** Scrollable `<div class="stream">` that renders one `<Row>` per record in `state.filtered()`. Also tracks scroll position → fires `on_scroll_not_at_bottom` when user scrolls up.

**Files:** Modify `ui/sift-ui/src/components/stream.rs`.

- [ ] **Step 1: Replace `stream.rs`**

```rust
//! Stream — scrollable log container. One <Row> per filtered record.
//! Fires `on_scroll_not_at_bottom` when user scrolls away from bottom.

use leptos::prelude::*;
use web_sys::HtmlDivElement;

use crate::app::AppCtx;
use crate::components::row::Row;

const SCROLL_BOTTOM_THRESHOLD: f64 = 4.0; // px from bottom still counts as "at bottom"

#[component]
pub fn Stream() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let stream_ref = NodeRef::<leptos::html::Div>::new();

    // Reactive iteration over filtered records.
    let rows = move || {
        state.with(|s| {
            s.filtered()
                .iter()
                .enumerate()
                .map(|(idx, r)| (idx, (*r).clone()))
                .collect::<Vec<_>>()
        })
    };

    view! {
        <div class="stream"
             node_ref=stream_ref
             on:scroll=move |_| {
                 if let Some(el) = stream_ref.get() {
                     let scroll_top = el.scroll_top() as f64;
                     let client_h = el.client_height() as f64;
                     let scroll_h = el.scroll_height() as f64;
                     let at_bottom = (scroll_h - (scroll_top + client_h)) <= SCROLL_BOTTOM_THRESHOLD;
                     state.update(|s| {
                         if !at_bottom {
                             // scroll up detected
                             s.on_scroll_not_at_bottom();
                         } else {
                             s.at_bottom = true;
                             // note: do NOT auto-resume live (hybrid C)
                         }
                     });
                 }
             }>
            <For each=rows
                 key=|(i, _)| *i
                 children=move |(idx, record)| {
                     view! { <Row idx=idx record=record/> }
                 }/>
        </div>
    }
}
```

**Leptos 0.8 notes:**
- `NodeRef` is `NodeRef::<leptos::html::Div>::new()` in 0.8 (changed from 0.6's `create_node_ref`).
- `<For>` keyed iteration takes `each`, `key`, `children` props (verify 0.8 signature).
- `node_ref=...` is the attribute on the HTML element.

If those exact names differ in 0.8, adapt. The LEPTOS 0.8 MIGRATION GUIDE <https://book.leptos.dev/> is authoritative.

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/stream.rs
git commit -m "feat(ui): stream component — scrollable container + scroll-up auto-pause"
```

---

## Task 11: `components/row.rs` — single log row

**Goal:** Renders one log row (date, host, app, lvl, body). Click → open detail for this row's index. Applies level-specific CSS class (`row warn`, `row error`, etc.).

**Files:** Modify `ui/sift-ui/src/components/row.rs`.

- [ ] **Step 1: Replace `row.rs`**

```rust
//! One log row — grid columns: date / host / app / lvl / body. Click
//! to expand inline detail.

use leptos::prelude::*;
use sift_core::{Level, Record};

use crate::app::AppCtx;
use crate::components::row_detail::RowDetail;
use crate::state::TimestampMode;

#[component]
pub fn Row(idx: usize, record: Record) -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let is_selected = move || state.with(|s| s.selected_row_index == Some(idx));
    let is_expanded = move || state.with(|s| s.detail_expanded && s.selected_row_index == Some(idx));
    let ts_mode = move || state.with(|s| s.timestamp_mode);

    let level_class = match record.level {
        Level::Info => "row",
        Level::Warn => "row warn",
        Level::Error => "row error",
        Level::Fatal => "row fatal",
        Level::Debug => "row debug",
        Level::Trace => "row", // could add trace class if styled
    };

    let row_class = move || {
        if is_selected() {
            format!("{level_class} selected")
        } else {
            level_class.to_string()
        }
    };

    let record_for_detail = record.clone();
    let record_for_ts = record.clone();
    let record_for_row = record.clone();

    view! {
        <>
            <div class=row_class
                 on:click=move |_| state.update(|s| {
                     if s.selected_row_index == Some(idx) && s.detail_expanded {
                         s.close_detail();
                     } else {
                         s.open_detail(idx);
                     }
                 })>
                <span class="date">{move || format_ts(&record_for_ts.timestamp, ts_mode())}</span>
                <span class="host">{record_for_row.host.clone()}</span>
                <span class="app">{record_for_row.app.clone()}</span>
                <span class="lvl">{level_str(record_for_row.level)}</span>
                <span class="body">{record_for_row.body.clone()}</span>
            </div>
            <Show when=is_expanded>
                <RowDetail record=record_for_detail.clone()/>
            </Show>
        </>
    }
}

fn level_str(l: Level) -> &'static str {
    match l {
        Level::Trace => "trace",
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
        Level::Fatal => "fatal",
    }
}

fn format_ts(ts: &chrono::DateTime<chrono::Utc>, mode: TimestampMode) -> String {
    match mode {
        TimestampMode::Absolute => ts.format("%H:%M:%S%.3f").to_string(),
        TimestampMode::Relative => {
            let now = chrono::Utc::now();
            let dur = now.signed_duration_since(*ts);
            let s = dur.num_seconds().max(0);
            if s < 60 { format!("{s}s ago") }
            else if s < 3600 { format!("{}m ago", s / 60) }
            else if s < 86400 { format!("{}h ago", s / 3600) }
            else { format!("{}d ago", s / 86400) }
        }
    }
}
```

**Leptos 0.8 notes:** `<Show when=... />` still exists. The `view! { <>...</> }` fragment syntax may need `<Fragment>` wrapping in 0.8; verify.

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/row.rs
git commit -m "feat(ui): row component — date/host/app/lvl/body + click-to-expand"
```

---

## Task 12: `components/row_detail.rs` — inline expand detail

**Goal:** Renders under the selected row: full body (wrapped), labels, fields (jsonfmt-highlighted), raw.

**Files:** Modify `ui/sift-ui/src/components/row_detail.rs`.

- [ ] **Step 1: Replace `row_detail.rs`**

```rust
//! Inline expand detail below a selected row.

use leptos::prelude::*;
use sift_core::Record;

use crate::app::AppCtx;
use crate::jsonfmt::{tokenize, JsonColor};

#[component]
pub fn RowDetail(record: Record) -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let labels_view = record.labels.iter()
        .map(|(k, v)| {
            let k = k.clone();
            let v = v.clone();
            view! { <div class="field-row"><span class="field-k">{k}</span><span class="field-v">{v}</span></div> }
        })
        .collect::<Vec<_>>();

    // fields JSON via jsonfmt
    let fields_json = serde_json::to_value(&record.fields)
        .unwrap_or(serde_json::Value::Null);
    let tokens = tokenize(&fields_json);
    let fields_view = tokens.into_iter()
        .map(|tok| {
            let class = match tok.color {
                JsonColor::Key => "json-k",
                JsonColor::Str => "json-s",
                JsonColor::Num => "json-n",
                JsonColor::Bool => "json-b",
                JsonColor::Null => "json-null",
                JsonColor::Punct => "json-p",
            };
            view! { <span class=class>{tok.text}</span> }
        })
        .collect::<Vec<_>>();

    let raw_text = record.raw.clone().unwrap_or_else(|| "(none)".to_string());

    view! {
        <div class="row-detail">
            <div class="detail-head">
                <span class="detail-title">"Detail"</span>
                <span class="detail-close"
                      on:click=move |_| state.update(|s| s.close_detail())>
                    "×"
                </span>
            </div>
            <div class="detail-sec">
                <div class="field-label">"BODY"</div>
                <div class="field-body">{record.body.clone()}</div>
            </div>
            <div class="detail-sec">
                <div class="field-label">"LABELS"</div>
                <div class="labels">{labels_view}</div>
            </div>
            <div class="detail-sec">
                <div class="field-label">"FIELDS"</div>
                <pre class="json">{fields_view}</pre>
            </div>
            <div class="detail-sec">
                <div class="field-label">"RAW"</div>
                <div class="field-raw">{raw_text}</div>
            </div>
        </div>
    }
}
```

- [ ] **Step 2: Append detail-specific styles**

Open `ui/sift-ui/styles.css` and append to the end:

```css
.row-detail {
    background: var(--panel);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    padding: 10px 16px;
    margin: 2px 0;
    font-size: 12px;
}
.detail-head { display: flex; align-items: center; padding-bottom: 8px; }
.detail-title { color: var(--text); font-weight: 500; flex: 1; }
.detail-close { color: var(--text-dim); cursor: pointer; padding: 0 6px; }
.detail-close:hover { color: var(--text); }
.detail-sec { margin: 10px 0; }
.field-label { color: var(--text-muted); font-size: 10.5px; text-transform: uppercase; letter-spacing: 0.7px; margin-bottom: 4px; }
.field-body { color: var(--text); white-space: pre-wrap; word-break: break-word; font-family: ui-monospace, Menlo, Consolas, monospace; padding: 4px 0; }
.field-raw { color: var(--text-muted); white-space: pre-wrap; font-family: ui-monospace, Menlo, Consolas, monospace; padding: 4px 0; font-size: 11.5px; }
.field-row { display: flex; gap: 8px; padding: 2px 0; font-family: ui-monospace, Menlo, Consolas, monospace; }
.field-k { color: var(--accent); min-width: 80px; }
.field-v { color: var(--text); }
.json { background: var(--bg); border: 1px solid var(--border); border-radius: 4px; padding: 10px 12px; margin: 0; font-family: ui-monospace, Menlo, Consolas, monospace; font-size: 11.5px; overflow-x: auto; white-space: pre; }
.json-k { color: var(--accent); }
.json-s { color: var(--ok); }
.json-n { color: var(--warn); }
.json-b { color: var(--debug); }
.json-null { color: var(--text-dim); }
.json-p { color: var(--text-muted); }
```

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/row_detail.rs ui/sift-ui/styles.css
git commit -m "feat(ui): row_detail — inline expand with jsonfmt-highlighted fields"
```

---

## Task 13: `components/paused_banner.rs` — floating resume pill

**Goal:** Fixed-position pill that appears when `!live`. Shows "Paused · N new records · Resume live". Click → `resume_live()`.

**Files:** Modify `ui/sift-ui/src/components/paused_banner.rs`.

- [ ] **Step 1: Replace `paused_banner.rs`**

```rust
//! Paused banner — fixed floating pill, visible only when !live.

use leptos::prelude::*;

use crate::app::AppCtx;

#[component]
pub fn PausedBanner() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let live = move || state.with(|s| s.live);
    let pending = move || state.with(|s| s.pending_count);

    view! {
        <Show when=move || !live()>
            <div class="resume"
                 on:click=move |_| state.update(|s| s.resume_live())>
                <span class="d"></span>
                <span>"Paused · " <span class="n">{move || pending()}</span> " new records"</span>
                <span class="play">"▶ Resume live"</span>
            </div>
        </Show>
    }
}
```

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/paused_banner.rs
git commit -m "feat(ui): paused_banner — resume pill, click to drain + live"
```

---

## Task 14: `components/search_row.rs` — bottom query input

**Goal:** Full-width query input at the bottom. `/` shortcut focuses it. Typing updates `state.query`. `:commands` button to the right opens the command modal.

**Files:** Modify `ui/sift-ui/src/components/search_row.rs`.

- [ ] **Step 1: Replace `search_row.rs`**

```rust
//! Bottom search / filter input. `/` focuses; `:commands` button
//! opens the command palette.

use leptos::prelude::*;

use crate::app::AppCtx;

#[component]
pub fn SearchRow() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let input_ref = NodeRef::<leptos::html::Input>::new();

    let on_input = move |e: leptos::ev::Event| {
        let value = event_target_value(&e);
        state.update(|s| s.query = value);
    };

    view! {
        <div class="search-row">
            <div class="query">
                <span class="ic">"⌕"</span>
                <input type="text"
                       placeholder="search / filter — e.g. level:error app:api-prod duration>5s"
                       node_ref=input_ref
                       on:input=on_input/>
                <span class="kbd">"/"</span>
            </div>
            <div class="cmd"
                 on:click=move |_| state.update(|s| s.open_command_prompt())>
                <span class="kbd">":"</span>"commands"
            </div>
        </div>
    }
}
```

**Leptos 0.8 note:** `event_target_value` is in `leptos::prelude::*`. `leptos::ev::Event` is the ergonomic event type.

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/search_row.rs
git commit -m "feat(ui): search_row — bottom query input + :commands button"
```

---

## Task 15: `components/status_bar.rs` — connection / records / window / hint

**Goal:** 24px strip at the very bottom. Shows connection status, record count, window, and `?` help hint.

**Files:** Modify `ui/sift-ui/src/components/status_bar.rs`.

- [ ] **Step 1: Replace `status_bar.rs`**

```rust
//! Bottom status strip — 24px tall, non-interactive info line.

use leptos::prelude::*;

use crate::app::AppCtx;
use crate::state::Status;

#[component]
pub fn StatusBar() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let status = move || state.with(|s| s.status);
    let records = move || state.with(|s| s.records.len());

    let status_text = move || match status() {
        Status::Connected => ("connected".to_string(), "ok"),
        Status::Connecting => ("connecting…".to_string(), "connecting"),
        Status::Disconnected => ("disconnected".to_string(), "err"),
    };

    view! {
        <div class="status">
            {move || {
                let (txt, klass) = status_text();
                view! {
                    <span class=format!("status-dot {klass}")>{txt}</span>
                }
            }}
            <span class="dot">"·"</span>
            <span>{move || format!("{} records", records())}</span>
            <span class="dot">"·"</span>
            <span>"window: Last 1 h"</span>
            <span class="spacer"></span>
            <span>"press "<span class="kbd">"?"</span>" for keyboard shortcuts"</span>
        </div>
    }
}
```

- [ ] **Step 2: Append status-dot styles**

Append to `ui/sift-ui/styles.css`:

```css
.status-dot.ok { color: var(--ok); }
.status-dot.ok::before { content:""; display:inline-block; width:5px; height:5px; border-radius:50%; background: var(--ok); margin-right: 5px; vertical-align: middle; }
.status-dot.connecting { color: var(--warn); }
.status-dot.connecting::before { content:""; display:inline-block; width:5px; height:5px; border-radius:50%; background: var(--warn); margin-right: 5px; vertical-align: middle; }
.status-dot.err { color: var(--error); }
.status-dot.err::before { content:""; display:inline-block; width:5px; height:5px; border-radius:50%; background: var(--error); margin-right: 5px; vertical-align: middle; }
.status .kbd { border: 1px solid var(--border); padding: 0 4px; border-radius: 3px; color: var(--text-muted); font-family: ui-monospace, Menlo, Consolas, monospace; font-size: 10.5px; }
```

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/status_bar.rs ui/sift-ui/styles.css
git commit -m "feat(ui): status_bar — connection + records + window + help hint"
```

---

## Task 16: `components/help_modal.rs` — `?` overlay

**Goal:** Modal rendered when `state.help_open`. Iterates `KEYBIND_TABLE` to render sections.

**Files:** Modify `ui/sift-ui/src/components/help_modal.rs`.

- [ ] **Step 1: Replace `help_modal.rs`**

```rust
//! Help overlay (`?`). Reads from KEYBIND_TABLE for single source of truth.

use leptos::prelude::*;

use crate::app::AppCtx;
use crate::keys::KEYBIND_TABLE;

#[component]
pub fn HelpModal() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let open = move || state.with(|s| s.help_open);

    let sections = KEYBIND_TABLE.iter()
        .map(|(title, entries)| {
            let entries_view = entries.iter()
                .map(|(k, v)| view! {
                    <div class="help-row">
                        <span class="help-k">{*k}</span>
                        <span class="help-v">{*v}</span>
                    </div>
                })
                .collect::<Vec<_>>();
            view! {
                <div class="help-section">
                    <h3 class="help-title">{*title}</h3>
                    <div class="help-entries">{entries_view}</div>
                </div>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <Show when=open>
            <div class="help-overlay"
                 on:click=move |_| state.update(|s| s.help_open = false)>
                <div class="help-modal" on:click=|e| e.stop_propagation()>
                    <div class="help-head">
                        <h2>"Keybindings"</h2>
                        <span class="help-close"
                              on:click=move |_| state.update(|s| s.help_open = false)>"×"</span>
                    </div>
                    <div class="help-body">{sections.clone()}</div>
                </div>
            </div>
        </Show>
    }
}
```

- [ ] **Step 2: Append help-modal styles**

Append to `ui/sift-ui/styles.css`:

```css
.help-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.6); display: flex; align-items: center; justify-content: center; z-index: 100; }
.help-modal { background: var(--bg-2); border: 1px solid var(--border-focus); border-radius: 6px; width: 640px; max-width: 92vw; max-height: 80vh; display: flex; flex-direction: column; box-shadow: 0 12px 40px rgba(0,0,0,0.6); overflow: hidden; }
.help-head { display: flex; align-items: center; padding: 12px 16px; border-bottom: 1px solid var(--border); }
.help-head h2 { margin: 0; flex: 1; font-size: 14px; color: var(--text); font-weight: 500; }
.help-close { color: var(--text-muted); cursor: pointer; font-size: 16px; }
.help-body { padding: 14px 16px; overflow-y: auto; }
.help-section { margin-bottom: 14px; }
.help-title { margin: 0 0 6px; font-size: 11px; color: var(--accent); text-transform: uppercase; letter-spacing: 0.8px; font-weight: 600; }
.help-row { display: grid; grid-template-columns: 240px 1fr; gap: 10px; padding: 3px 0; font-size: 12.5px; }
.help-k { color: var(--text); font-family: ui-monospace, Menlo, Consolas, monospace; }
.help-v { color: var(--text-muted); }
```

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/help_modal.rs ui/sift-ui/styles.css
git commit -m "feat(ui): help_modal — ? overlay backed by KEYBIND_TABLE"
```

---

## Task 17: `components/command_modal.rs` — `:` palette

**Goal:** Modal with an input field. Typing updates `state.command_input`. Enter parses via `command::parse()`, dispatches the action, closes. Esc cancels. Tab completes the verb prefix.

**Files:** Modify `ui/sift-ui/src/components/command_modal.rs`.

- [ ] **Step 1: Replace `command_modal.rs`**

```rust
//! `:` command palette modal. Mirrors the TUI's command prompt.

use leptos::prelude::*;

use crate::app::AppCtx;
use crate::command::{complete, parse, CommandAction};

#[component]
pub fn CommandModal() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let open = move || state.with(|s| s.command_open);
    let value = move || state.with(|s| s.command_input.clone());

    let on_input = move |e: leptos::ev::Event| {
        let v = event_target_value(&e);
        state.update(|s| s.command_input = v);
    };

    let on_keydown = move |e: leptos::ev::KeyboardEvent| {
        let key = e.key();
        match key.as_str() {
            "Escape" => {
                e.prevent_default();
                state.update(|s| s.close_command_prompt());
            }
            "Enter" => {
                e.prevent_default();
                let input = state.with(|s| s.command_input.clone());
                match parse(&input) {
                    Ok(action) => {
                        state.update(|s| {
                            s.pending_command = Some(action);
                            s.close_command_prompt();
                        });
                    }
                    Err(err) => {
                        state.update(|s| s.set_flash_err(format!("bad command: {err}")));
                    }
                }
            }
            "Tab" => {
                e.prevent_default();
                let input = state.with(|s| s.command_input.clone());
                let c = complete(&input);
                if !c.prefix.is_empty() {
                    state.update(|s| s.command_input = c.prefix);
                }
            }
            _ => {}
        }
    };

    view! {
        <Show when=open>
            <div class="cmd-overlay"
                 on:click=move |_| state.update(|s| s.close_command_prompt())>
                <div class="cmd-modal" on:click=|e| e.stop_propagation()>
                    <span class="cmd-prefix">":"</span>
                    <input type="text"
                           class="cmd-input"
                           prop:value=value
                           on:input=on_input
                           on:keydown=on_keydown
                           autofocus="autofocus"/>
                </div>
            </div>
        </Show>
    }
}
```

- [ ] **Step 2: Append command-modal styles**

Append to `ui/sift-ui/styles.css`:

```css
.cmd-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.4); display: flex; align-items: flex-end; justify-content: center; padding-bottom: 72px; z-index: 200; }
.cmd-modal { background: var(--bg-2); border: 1px solid var(--border-focus); border-radius: 6px; width: 560px; max-width: 90vw; display: flex; align-items: center; padding: 8px 12px; gap: 6px; box-shadow: 0 12px 40px rgba(0,0,0,0.6); }
.cmd-prefix { color: var(--accent); font-family: ui-monospace, Menlo, Consolas, monospace; font-weight: 600; font-size: 14px; }
.cmd-input { flex: 1; background: transparent; border: none; color: var(--text); font-family: ui-monospace, Menlo, Consolas, monospace; font-size: 13px; outline: none; }
```

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/command_modal.rs ui/sift-ui/styles.css
git commit -m "feat(ui): command_modal — : palette with Enter/Esc/Tab + parse"
```

---

## Task 18: `ws.rs` — WebSocket adapt to AppState

**Goal:** Replace the stub with real WebSocket handling. Connect on mount, auto-reconnect with exponential backoff, push records to `AppState`.

**Files:** Modify `ui/sift-ui/src/ws.rs`, `ui/sift-ui/src/app.rs`.

- [ ] **Step 1: Replace `ws.rs`**

```rust
//! WebSocket client — connects to /v1/tail, pushes records into AppState.
//! Auto-reconnects with exponential backoff (1s → 30s cap).

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{MessageEvent, WebSocket};

use crate::app::AppCtx;
use crate::state::Status;

pub fn connect_forever(ctx: AppCtx) {
    spawn_local(async move {
        let mut attempt: u32 = 0;
        loop {
            ctx.0.update(|s| s.status = Status::Connecting);

            match try_connect(ctx).await {
                Ok(()) => {
                    // Connection opened and then closed normally — reset attempt.
                    attempt = 0;
                }
                Err(_) => {
                    attempt = (attempt + 1).min(5);
                }
            }
            ctx.0.update(|s| s.status = Status::Disconnected);

            let backoff_ms = (1u32 << attempt).saturating_mul(1000).min(30_000);
            TimeoutFuture::new(backoff_ms).await;
        }
    });
}

async fn try_connect(ctx: AppCtx) -> Result<(), ()> {
    let url = ws_url();
    let ws = WebSocket::new(&url).map_err(|_| ())?;

    let ctx_open = ctx;
    let onopen = Closure::<dyn FnMut()>::new(move || {
        ctx_open.0.update(|s| s.status = Status::Connected);
    });
    ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let ctx_msg = ctx;
    let onmessage = Closure::<dyn FnMut(MessageEvent)>::new(move |evt: MessageEvent| {
        if let Some(data) = evt.data().as_string() {
            if let Ok(record) = serde_json::from_str::<sift_core::Record>(&data) {
                ctx_msg.0.update(|s| s.push_record(record));
            }
        }
    });
    ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();

    // Hold connection open until it closes. We don't await — the future
    // loop in connect_forever exits when onclose fires. For simplicity,
    // just sleep indefinitely here and let the WS handle its own lifecycle.
    //
    // A cleaner version would use a oneshot channel from onclose to here.
    futures::future::pending::<()>().await;
    Ok(())
}

fn ws_url() -> String {
    let loc = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://localhost:7789".to_string());
    let ws = loc.replace("http://", "ws://").replace("https://", "wss://");
    format!("{ws}/v1/tail")
}
```

**Note:** this simplifies the existing backoff logic. The existing `ws.rs` has more nuance (stale-attempt bug fix from UX v2 task 17). Preserve the `did_open` pattern: reset `attempt` only after a successful `onopen`, not just "try_connect returned Ok". If you want faithfulness to the prior fix, copy that piece from the pre-T5 `ws.rs` git history:

```bash
git show HEAD~5:ui/sift-ui/src/ws.rs > /tmp/old_ws.rs
# Study the did_open pattern and replicate.
```

- [ ] **Step 2: Call `connect_forever` from `app.rs`**

Add to `App()` after `provide_context`:

```rust
    // Kick off WS connection.
    crate::ws::connect_forever(AppCtx(state));
```

- [ ] **Step 3: Build + smoke**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cd ui/sift-ui && trunk build && cd ../..
```

- [ ] **Step 4: Format + clippy + commit**

```bash
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/ws.rs ui/sift-ui/src/app.rs
git commit -m "feat(ui): ws — connect to /v1/tail + backoff + push to AppState"
```

---

## Task 19: `query.rs` — `/v1/search` fetch helper

**Goal:** Async helper to fire windowed / backward-pagination queries against `/v1/search`. Returns records.

**Files:** Create `ui/sift-ui/src/query.rs`, modify `main.rs`.

- [ ] **Step 1: Create `query.rs`**

```rust
//! Backend query helper — wraps /v1/search for windowed queries and
//! backward pagination.

use chrono::{DateTime, Utc};
use gloo_net::http::Request;
use sift_core::Record;

#[derive(Debug)]
pub enum QueryError {
    Http(String),
    Parse(String),
}

pub async fn fetch_window(
    q: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<Record>, QueryError> {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "http://localhost:7789".to_string());

    let url = format!(
        "{origin}/v1/search?q={}&from={}&to={}&limit={}",
        urlencoded(q),
        from.to_rfc3339(),
        to.to_rfc3339(),
        limit,
    );

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| QueryError::Http(e.to_string()))?;

    if !resp.ok() {
        return Err(QueryError::Http(format!("HTTP {}", resp.status())));
    }

    resp.json::<Vec<Record>>()
        .await
        .map_err(|e| QueryError::Parse(e.to_string()))
}

fn urlencoded(s: &str) -> String {
    js_sys::encode_uri_component(s).as_string().unwrap_or_default()
}
```

- [ ] **Step 2: Register module**

In `main.rs`:

```rust
mod query;
```

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/query.rs ui/sift-ui/src/main.rs
git commit -m "feat(ui): query — /v1/search fetch helper (async)"
```

---

## Task 20: Global keyboard dispatcher

**Goal:** Document-level keydown listener. Routes keys to state methods per KEYBIND_TABLE.

**Files:** Modify `ui/sift-ui/src/app.rs`.

- [ ] **Step 1: Add keydown handler inside `App()` component**

```rust
use leptos::ev::KeyboardEvent;
use wasm_bindgen::JsCast;

// Inside fn App() {}, after provide_context + ws::connect_forever:

    let state_for_keys = state;
    let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        handle_global_key(state_for_keys, &e);
    });
    if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
        let _ = doc.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        cb.forget();
    }
```

And the `handle_global_key` function:

```rust
fn handle_global_key(state: RwSignal<AppState>, e: &KeyboardEvent) {
    let key = e.key();
    let ctrl = e.ctrl_key();

    // Don't swallow keys when user is typing in an <input>.
    let in_input = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .map(|el| el.tag_name() == "INPUT" || el.tag_name() == "TEXTAREA")
        .unwrap_or(false);

    match (key.as_str(), ctrl, in_input) {
        ("Escape", _, _) => {
            e.prevent_default();
            state.update(|s| {
                if s.command_open { s.close_command_prompt(); }
                else if s.help_open { s.help_open = false; }
                else if s.detail_expanded { s.close_detail(); }
            });
        }
        ("?", false, false) => {
            e.prevent_default();
            state.update(|s| s.toggle_help());
        }
        (":", false, false) => {
            e.prevent_default();
            state.update(|s| s.open_command_prompt());
        }
        ("/", false, false) => {
            e.prevent_default();
            if let Some(input) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(".query input").ok().flatten())
                .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok()) {
                let _ = input.focus();
            }
        }
        ("t", true, _) => {
            e.prevent_default();
            state.update(|s| s.toggle_timestamp_mode());
        }
        ("G" | "End", false, false) => {
            e.prevent_default();
            state.update(|s| s.resume_live());
        }
        (" ", false, false) => {
            e.prevent_default();
            state.update(|s| {
                if s.live { s.live = false; }
                else { s.resume_live(); }
            });
        }
        ("I", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Info)),
        ("W", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Warn)),
        ("E", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Error)),
        ("F", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Fatal)),
        ("D", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Debug)),
        ("T", false, false) => state.update(|s| s.toggle_level(sift_core::Level::Trace)),
        _ => {}
    }
}
```

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/app.rs
git commit -m "feat(ui): global keyboard dispatcher — /:?Esc/GG/Ctrl+T/I-W-E-F-D-T/Space"
```

---

## Task 21: Dispatch `pending_command` in app shell

**Goal:** Drain `state.pending_command` each tick; execute the action.

**Files:** Modify `ui/sift-ui/src/app.rs`.

- [ ] **Step 1: Add a reactive effect in `App()`**

```rust
    // Dispatch pending commands.
    Effect::new(move |_| {
        if let Some(action) = state.with(|s| s.pending_command.clone()) {
            dispatch_command(state, action);
            state.update(|s| s.pending_command = None);
        }
    });
```

And `dispatch_command`:

```rust
fn dispatch_command(state: RwSignal<AppState>, action: CommandAction) {
    use crate::command::CommandAction as A;
    match action {
        A::Pause => state.update(|s| { s.live = false; s.set_flash_ok("paused"); }),
        A::Resume => state.update(|s| { s.resume_live(); s.set_flash_ok("resumed"); }),
        A::Clear => state.update(|s| s.clear_records()),
        A::Retry => {
            // Kick reconnect by toggling status — actual reconnect handled by ws loop.
            state.update(|s| { s.status = Status::Disconnected; s.set_flash_ok("retry triggered"); });
        }
        A::Save { path } => {
            // Serialize records → NDJSON → browser download via synthetic <a download>.
            let records: Vec<sift_core::Record> = state.with(|s| s.records.iter().cloned().collect());
            let mut body = String::new();
            for r in &records {
                if let Ok(line) = serde_json::to_string(r) {
                    body.push_str(&line);
                    body.push('\n');
                }
            }
            let filename = path.file_name()
                .map(|os| os.to_string_lossy().into_owned())
                .unwrap_or_else(|| "sift-records.ndjson".to_string());
            if let Err(e) = trigger_download(&body, &filename) {
                state.update(|s| s.set_flash_err(format!("save failed: {e}")));
            } else {
                state.update(|s| s.set_flash_ok(format!("saved {} records to {}", records.len(), filename)));
            }
        }
        A::Search { q, from: _, to: _ } => {
            state.update(|s| {
                if let Some(q) = q { s.query = q; }
                s.set_flash_ok("search updated");
            });
        }
        A::Level { level, on } => {
            state.update(|s| {
                let current = s.level_set.allows(level);
                if current != on { s.toggle_level(level); }
                s.set_flash_ok(format!("{level:?} {}", if on { "on" } else { "off" }));
            });
        }
        A::Help => state.update(|s| s.help_open = true),
        A::Quit => {
            if let Some(w) = web_sys::window() { let _ = w.close(); }
        }
    }
}

/// Trigger a browser download of `body` as a file named `filename`.
fn trigger_download(body: &str, filename: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast;

    let blob_parts = js_sys::Array::new();
    blob_parts.push(&wasm_bindgen::JsValue::from_str(body));
    let blob_opts = web_sys::BlobPropertyBag::new();
    blob_opts.set_type("application/x-ndjson");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&blob_parts, &blob_opts)
        .map_err(|_| "blob create failed".to_string())?;

    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|_| "object url failed".to_string())?;

    let doc = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| "no document".to_string())?;
    let a = doc.create_element("a")
        .map_err(|_| "a create failed".to_string())?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "not an anchor".to_string())?;
    a.set_href(&url);
    a.set_download(filename);
    a.click();

    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}
```

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/app.rs
git commit -m "feat(ui): dispatch pending_command — wire all 9 palette verbs"
```

---

## Task 22: Flash display above search row

**Goal:** When `state.flash` is Some, render a floating banner above the search row showing the message in green (Ok) or red (Err). Auto-expires at the timestamp stored in `flash.expires_at_ms`.

**Files:** Modify `ui/sift-ui/src/app.rs` to add a `<Flash/>` component OR inline it.

- [ ] **Step 1: Create `components/flash.rs`**

```rust
//! Flash message — floating banner above the search row.

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::app::AppCtx;
use crate::state::FlashKind;

#[component]
pub fn Flash() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    // Tick effect: every 500ms, call expire_flash_if_due.
    Effect::new(move |_| {
        spawn_local(async move {
            loop {
                TimeoutFuture::new(500).await;
                state.update(|s| s.expire_flash_if_due());
            }
        });
    });

    let flash = move || state.with(|s| s.flash.clone());

    view! {
        <Show when=move || flash().is_some()>
            {move || {
                let f = flash().unwrap();
                let klass = match f.kind {
                    FlashKind::Ok => "flash ok",
                    FlashKind::Err => "flash err",
                };
                view! {
                    <div class=klass>{f.msg}</div>
                }
            }}
        </Show>
    }
}
```

Append styles:

```css
.flash { position: fixed; bottom: 76px; left: calc(50% + 20px); transform: translateX(-50%); padding: 7px 14px; border-radius: 999px; font-size: 12px; z-index: 10; box-shadow: 0 6px 20px rgba(0,0,0,0.5); }
.flash.ok { background: rgba(107, 212, 155, 0.18); color: var(--ok); border: 1px solid rgba(107, 212, 155, 0.35); }
.flash.err { background: rgba(255, 107, 107, 0.18); color: var(--error); border: 1px solid rgba(255, 107, 107, 0.35); }
```

- [ ] **Step 2: Register + use in `app.rs`**

Add `pub mod flash;` to `components/mod.rs`. In `Home()`, add `<Flash/>` at the end.

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/flash.rs ui/sift-ui/src/components/mod.rs ui/sift-ui/src/app.rs ui/sift-ui/styles.css
git commit -m "feat(ui): flash component — auto-expire in-UI status messages"
```

---

## Task 23: Time range picker dropdown

**Goal:** Clicking the "Last 1 h ▾" pill opens a dropdown with presets. Selecting a preset updates `time_window`.

**Files:** Modify `ui/sift-ui/src/components/top_bar.rs`.

- [ ] **Step 1: Add a `range_open` local signal + dropdown markup**

Replace the `<div class="range">...</div>` block in `top_bar.rs` with:

```rust
let range_open = RwSignal::new(false);

// Inside the view!:
<div class="range-wrap">
    <div class="range"
         on:click=move |_| range_open.update(|o| *o = !*o)>
        <span class="ic">"◷"</span>
        <span>{move || window_label(state)}</span>
        <span class="chev">"▾"</span>
    </div>
    <Show when=move || range_open.get()>
        <div class="range-dropdown">
            {preset_item("Last 15 min", 900, state, range_open)}
            {preset_item("Last 1 h", 3600, state, range_open)}
            {preset_item("Last 6 h", 21_600, state, range_open)}
            {preset_item("Last 24 h", 86_400, state, range_open)}
            {preset_item("Last 7 d", 604_800, state, range_open)}
        </div>
    </Show>
</div>
```

Helper functions (outside the component):

```rust
fn window_label(state: RwSignal<AppState>) -> String {
    state.with(|s| match &s.time_window {
        TimeWindow::Preset(d) => {
            let secs = d.as_secs();
            if secs == 900 { "Last 15 min".to_string() }
            else if secs == 3600 { "Last 1 h".to_string() }
            else if secs == 21_600 { "Last 6 h".to_string() }
            else if secs == 86_400 { "Last 24 h".to_string() }
            else if secs == 604_800 { "Last 7 d".to_string() }
            else { format!("Last {secs}s") }
        }
        TimeWindow::Custom { from, to } => format!("{} → {}",
            from.format("%m-%d %H:%M"), to.format("%m-%d %H:%M")),
    })
}

fn preset_item(
    label: &'static str,
    secs: u64,
    state: RwSignal<AppState>,
    open: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="range-item"
             on:click=move |_| {
                 state.update(|s| s.time_window = TimeWindow::Preset(std::time::Duration::from_secs(secs)));
                 open.set(false);
             }>
            {label}
        </div>
    }
}
```

Imports to add:
```rust
use crate::state::{AppState, TimeWindow};
```

- [ ] **Step 2: Append range-dropdown styles**

```css
.range-wrap { position: relative; }
.range-dropdown { position: absolute; top: 100%; left: 0; margin-top: 4px; background: var(--panel); border: 1px solid var(--border-focus); border-radius: 5px; padding: 4px 0; min-width: 160px; z-index: 50; box-shadow: 0 6px 20px rgba(0,0,0,0.4); }
.range-item { padding: 6px 12px; font-size: 12.5px; color: var(--text); cursor: pointer; }
.range-item:hover { background: var(--panel-2); }
```

- [ ] **Step 3: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/components/top_bar.rs ui/sift-ui/styles.css
git commit -m "feat(ui): time range dropdown — preset selection in top bar"
```

---

## Task 24: React to `time_window` changes — query fetch

**Goal:** When `time_window` changes (via the preset dropdown or a `:search` command), fire a backend query and replace records.

**Files:** Modify `ui/sift-ui/src/app.rs`.

- [ ] **Step 1: Add an effect that watches `time_window`**

In `App()`:

```rust
    // Effect: when time_window changes (or query changes while in windowed mode),
    // refetch from backend.
    Effect::new(move |prev: Option<crate::state::TimeWindow>| {
        let current = state.with(|s| s.time_window.clone());

        // Skip first run (prev is None).
        if let Some(p) = prev.as_ref() {
            if *p == current {
                return current;
            }
        }
        if prev.is_none() {
            return current;
        }

        // Fire backend query asynchronously.
        let current_snapshot = current.clone();
        spawn_local(async move {
            let (from, to) = window_bounds(&current_snapshot);
            let q = state.with(|s| s.query.clone());
            match crate::query::fetch_window(&q, from, to, 500).await {
                Ok(records) => {
                    state.update(|s| s.replace_records(records));
                }
                Err(e) => {
                    state.update(|s| s.set_flash_err(format!("query failed: {e:?}")));
                }
            }
        });

        current
    });
```

And `window_bounds`:

```rust
fn window_bounds(w: &crate::state::TimeWindow) -> (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) {
    match w {
        crate::state::TimeWindow::Preset(d) => {
            let to = chrono::Utc::now();
            let from = to - chrono::Duration::from_std(*d).unwrap_or_else(|_| chrono::Duration::hours(1));
            (from, to)
        }
        crate::state::TimeWindow::Custom { from, to } => (*from, *to),
    }
}
```

- [ ] **Step 2: Build + clippy + commit**

```bash
cargo build -p sift-ui --target wasm32-unknown-unknown
cargo fmt --all
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
git add ui/sift-ui/src/app.rs
git commit -m "feat(ui): refetch on time_window change — /v1/search fires automatically"
```

---

## Task 25: Final verification + README

**Goal:** Run the full build, smoke test against a real `sift-server`, update the README, and confirm success criteria.

**Files:** Modify `ui/sift-ui/README.md` (or `README.md` in the UI crate if present).

- [ ] **Step 1: Final clean build**

```bash
cd ui/sift-ui
trunk build --release
ls dist/  # should see hashed .wasm + .js + .css + index.html
```

- [ ] **Step 2: Start backend + serve UI**

Option A: use existing `deploy.ps1 -Ui` if applicable; Option B: `trunk serve` in one terminal and `cargo run -p sift-server` in another.

- [ ] **Step 3: Smoke test checklist**

Navigate to the served UI and verify:

- [ ] Page loads at `/`; `/tail` and `/search` redirect to `/`.
- [ ] LIVE indicator pulses green by default.
- [ ] Record rows appear as records stream in.
- [ ] Scroll up → LIVE becomes PAUSED; banner "Paused · N new records · Resume live" appears.
- [ ] Clicking banner OR pressing `G` resumes + jumps to bottom.
- [ ] Clicking a level pill hides/shows that level.
- [ ] Typing in the bottom search box filters client-side.
- [ ] `/` focuses search.
- [ ] `?` opens help; Esc closes; KEYBIND_TABLE sections render.
- [ ] `:` opens command palette; typing `pause` + Enter pauses; `:resume` resumes; `:clear` clears.
- [ ] Clicking a row expands inline detail with JSON-highlighted fields.
- [ ] `Ctrl+T` toggles between `12:34:56.789` and `2s ago`.
- [ ] Opening `/tail?q=error` → loads `/` with query pre-populated.
- [ ] Time range dropdown works; selecting "Last 24 h" fires a backend query.

- [ ] **Step 4: Update `ui/sift-ui/README.md`**

```markdown
# sift-ui

Leptos 0.8 (CSR) web UI for Sift — single unified log stream.

## Routes

Just `/`. Legacy `/tail` and `/search` URLs redirect to `/` on load, preserving `?q=` if present.

## Keyboard

See the in-app `?` overlay for the complete list. Highlights:

- `/` focus query, `:` open command palette, `?` help.
- `j`/`k`/`g`/`G` / arrows / Home / End for navigation.
- `Space` pause/resume. `G` resumes and jumps to latest.
- `I`/`W`/`E`/`F`/`D`/`T` toggle level visibility.
- `Ctrl+T` toggle absolute / relative timestamps.

## Command palette (`:`)

- `:pause` / `:resume` / `:clear` / `:retry`
- `:search q=<str> from=<iso> to=<iso>`
- `:level <name> on|off`
- `:help` / `:quit`

## Dev

```
cd ui/sift-ui
trunk serve
```

Needs `sift-server` running on the same origin.
```

- [ ] **Step 5: Commit README**

```bash
cd ../..
git add ui/sift-ui/README.md
git commit -m "docs(ui): README for v2 UI — routes, keybinds, :palette"
```

- [ ] **Step 6: Final clippy + test**

```bash
cargo test -p sift-ui
cargo clippy -p sift-ui --all-targets -- -D warnings -W clippy::pedantic
cargo clippy --workspace --all-targets -- -D warnings
```

All must be green.

---

## Final checklist (all 12 success criteria from spec §12)

1. [ ] `/` renders the unified stream; `/tail` and `/search` are gone.
2. [ ] Default: LIVE on, Last 1h window, records stream in.
3. [ ] Scroll up → pause; resume banner appears; click → return + live.
4. [ ] Time range change → `/v1/search` fires → records replaced; live off.
5. [ ] Level pills filter client-side.
6. [ ] `:` opens palette; all 9 verbs dispatch.
7. [ ] `?` opens help; same keybind table as TUI.
8. [ ] Row click → inline detail with jsonfmt-highlighted `fields`.
9. [ ] Keyboard shortcuts match the TUI (j/k/g/G/Enter/Space/I-W-E-F-D-T/Ctrl+T/`/`:/`?`).
10. [ ] `cargo build --target wasm32-unknown-unknown` + `trunk build --release` + end-to-end against `sift-server` all green.

If any unchecked: open a follow-up task, do not declare "done."
