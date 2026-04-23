# TUI Overhaul Implementation Plan — theme, panes, blocks, `:` command palette

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Ratatui TUI from a plain-text log view into a widget-composed app with focus-aware borders, a `:` command palette, colored level blocks, rate sparkline, and syntect-highlighted detail popup — matching the polish of openapi-tui / yozefu while preserving UX v2 feature parity.

**Architecture:** Introduce `theme.rs` (single `Theme` struct, dark default), `focus.rs` (`Pane` enum + cycle logic), `command.rs` (`:` palette parser + dispatcher), and `view_footer.rs` (mode badge + hints + flash). Refactor `view_tail`/`view_search`/`view_detail`/`view_help` to use `Block`/`Table`/`Sparkline`/`tui-input`/`throbber-widgets-tui`/`syntect`. Bump Ratatui 0.29 → 0.30.

**Tech Stack:** Rust 2021 · Ratatui 0.30 · crossterm 0.29 · tui-input 0.15 · syntect 5.3 · throbber-widgets-tui 0.11 · timeago 0.6 · tokio 1.40 · reqwest 0.12.

**Spec:** `docs/superpowers/specs/2026-04-23-tui-overhaul-design.md` (commit `18df358`).

---

## Ground rules for every task

- **One concern per commit.** Each task ends with a single commit.
- **TDD where practical.** Pure-state and pure-view tasks start with a failing test. Pure dep bumps and view integration work rely on compile + `cargo test` green as the gate.
- **Always run `cargo fmt --all`** after editing Rust files. Fold fmt hunks into the same commit.
- **Always run `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`** before committing. Adapt spec-provided code to pedantic nits where needed (document adaptations in the commit message or report).
- **`cargo test -p sift-cli`** must stay green after every task. Current baseline is 60 passing tests (post-UX v2).
- **Dep-freshness rule:** if during implementation a newer stable version has dropped, use it. Run `cargo update` after every `Cargo.toml` edit.
- **Never skip git hooks.** If a commit hook fails, fix the issue and create a new commit.

---

## File structure (target)

```
crates/sift-cli/src/tui/
├─ mod.rs               # lifecycle + event loop; gains 1 Hz rate tick arm
├─ state.rs             # grows: theme, focus_pane, timestamp_mode, command
│                       #        state, flash, rate ring
├─ theme.rs             # NEW — Theme struct + dark default
├─ focus.rs             # NEW — Pane enum + cycle logic
├─ command.rs           # NEW — command parser + dispatch plumbing
├─ input.rs             # grows: modal dispatcher (Normal/Insert/Command)
├─ record_input.rs      # NEW — tui-input wrapper used by filter + search inputs
├─ view.rs              # unchanged role: top-level draw dispatcher
├─ view_tail.rs         # REWRITTEN — Block outer, Table rows, tri-section bar
├─ view_search.rs       # REWRITTEN — bordered inputs + throbber spinner
├─ view_detail.rs       # REWRITTEN — nested sections + syntect JSON
├─ view_help.rs         # REWRITTEN — KEYBIND_TABLE two-column grid
├─ view_footer.rs       # NEW — mode badge + hints + command prompt + flash
├─ jsonfmt.rs           # KEPT — fallback when syntect fails
└─ ws.rs                # unchanged
```

---

## Task 1: Bump Ratatui 0.30 + crossterm 0.29

**Goal:** Upgrade the two terminal crates to the versions shared by openapi-tui and yozefu. No new features yet; ensure the existing 60 tests still pass against the new deps. Task is isolated so any 0.29 → 0.30 API drift surfaces here and nowhere else.

**Files:**
- Modify: `crates/sift-cli/Cargo.toml`
- Modify (if 0.30 drift forces it): `crates/sift-cli/src/tui/*.rs`

- [ ] **Step 1: Edit `crates/sift-cli/Cargo.toml`**

Replace the ratatui + crossterm lines:

```toml
ratatui = { version = "0.30", features = ["crossterm"] }
crossterm = { version = "0.29", features = ["event-stream"] }
```

(The `crossterm` feature on ratatui pulls `ratatui-crossterm`, which bridges ratatui 0.30 to the external crossterm crate.)

- [ ] **Step 2: `cargo update -p ratatui -p crossterm`**

Run: `cargo update -p ratatui -p crossterm` from the repo root.
Expected: lockfile updates to `ratatui 0.30.x` + `crossterm 0.29.x`.

- [ ] **Step 3: Try a build**

Run: `cargo check -p sift-cli`
Expected: either (a) clean compile, or (b) API drift errors.

- [ ] **Step 4: Fix API drift (if any)**

Known possible drift points from the 0.29 → 0.30 diff:

1. `Frame::size()` was deprecated in favour of `Frame::area()` in 0.29 already; if any code still uses `size()`, replace with `area()`.
2. `Block::title(Title::from("..."))` — in 0.30 the `Title` wrapper is moved to `ratatui::widgets::block::Title`. If the code just does `Block::default().title("foo")` that continues to work via `Into<Line>`; if anywhere uses explicit `Title`, re-import from the new path.
3. `Layout::default().direction(...).constraints([...])` — still works in 0.30; no change needed.
4. `KeyEvent::new(code, mods)` — unchanged in crossterm 0.29.

Grep each source file for patterns that might drift: `grep -rn "Frame::size\|widgets::block::Title\|\.split(" crates/sift-cli/src/tui/`. Fix any breakage in place.

- [ ] **Step 5: Run the test suite**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: 60 tests pass, 0 fail.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean. Fix any new lints introduced by 0.30's API.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/Cargo.toml Cargo.lock
# include any src/tui/ files that needed drift fixes
git commit -m "chore(tui): bump Ratatui 0.29 → 0.30 + crossterm 0.28 → 0.29"
```

Include any drift fixes in the commit message body if they weren't trivial.

---

## Task 2: Add tui-input, syntect, throbber-widgets-tui, timeago

**Goal:** Pin the four new deps at their current latest stable. Not consumed yet; later tasks pick them up.

**Files:**
- Modify: `crates/sift-cli/Cargo.toml`

- [ ] **Step 1: Edit `crates/sift-cli/Cargo.toml`**

Append to `[dependencies]`:

```toml
tui-input = "0.15"
syntect = "5.3"
throbber-widgets-tui = "0.11"
timeago = "0.6"
```

- [ ] **Step 2: `cargo update`**

Run: `cargo update -p tui-input -p syntect -p throbber-widgets-tui -p timeago`
Expected: each resolves to the expected major/minor.

- [ ] **Step 3: Build check**

Run: `cargo check -p sift-cli`
Expected: clean. (`unused_crate_dependencies` is not enabled in the workspace, so no warnings for the moment.)

- [ ] **Step 4: Tests still pass**

Run: `cargo test -p sift-cli`
Expected: 60 pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/Cargo.toml Cargo.lock
git commit -m "chore(tui): add tui-input / syntect / throbber-widgets-tui / timeago"
```

---

## Task 3: `theme.rs` — Theme struct + dark default

**Goal:** Single source of truth for every color the TUI uses. No rendering code yet — this task is data plus tests.

**Files:**
- Create: `crates/sift-cli/src/tui/theme.rs`
- Modify: `crates/sift-cli/src/tui/mod.rs` (register module)

- [ ] **Step 1: Create the module with a failing test**

Create `crates/sift-cli/src/tui/theme.rs`:

```rust
//! Theme — the single source of truth for every colour the TUI renders.
//!
//! One `Theme` instance lives in `AppState`. Views read slots off it rather
//! than hardcoding `Color::Cyan` etc., so re-skinning is a one-file change.

use ratatui::style::Color;
use sift_core::Level;

#[derive(Clone, Debug)]
pub struct Theme {
    // Base
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,

    // Borders
    pub border: Color,
    pub border_focused: Color,

    // Selection
    pub bg_selected: Color,
    pub fg_selected: Color,
    pub bg_selected_unfocused: Color,

    // Levels
    pub level_trace: Color,
    pub level_debug: Color,
    pub level_info: Color,
    pub level_warn: Color,
    pub level_error: Color,
    pub level_fatal: Color,

    // Semantic
    pub shortcut_key: Color,
    pub shortcut_label: Color,
    pub flash_ok: Color,
    pub flash_err: Color,
    pub disabled: Color,

    // Syntect integration
    pub highlighter_theme: &'static str,
}

impl Theme {
    #[must_use]
    pub fn dark() -> Self {
        Self {
            fg: Color::Rgb(215, 215, 215),
            muted: Color::Rgb(120, 120, 120),
            accent: Color::Cyan,

            border: Color::Rgb(60, 60, 60),
            border_focused: Color::Cyan,

            bg_selected: Color::Rgb(50, 50, 80),
            fg_selected: Color::Rgb(255, 255, 255),
            bg_selected_unfocused: Color::Rgb(40, 40, 50),

            level_trace: Color::DarkGray,
            level_debug: Color::Cyan,
            level_info: Color::Green,
            level_warn: Color::Yellow,
            level_error: Color::Red,
            level_fatal: Color::Magenta,

            shortcut_key: Color::Cyan,
            shortcut_label: Color::Gray,
            flash_ok: Color::Green,
            flash_err: Color::Red,
            disabled: Color::DarkGray,

            highlighter_theme: "base16-ocean.dark",
        }
    }

    #[must_use]
    pub fn level_color(&self, lvl: Level) -> Color {
        match lvl {
            Level::Trace => self.level_trace,
            Level::Debug => self.level_debug,
            Level::Info => self.level_info,
            Level::Warn => self.level_warn,
            Level::Error => self.level_error,
            Level::Fatal => self.level_fatal,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_default_has_every_slot_filled() {
        let t = Theme::dark();
        // Smoke-check a handful of slots to ensure they're not zero-initialised.
        assert_eq!(t.accent, Color::Cyan);
        assert_eq!(t.border_focused, Color::Cyan);
        assert_eq!(t.level_error, Color::Red);
        assert_eq!(t.flash_ok, Color::Green);
        assert_eq!(t.highlighter_theme, "base16-ocean.dark");
    }

    #[test]
    fn level_color_maps_every_level_to_its_slot() {
        let t = Theme::dark();
        assert_eq!(t.level_color(Level::Trace), t.level_trace);
        assert_eq!(t.level_color(Level::Debug), t.level_debug);
        assert_eq!(t.level_color(Level::Info), t.level_info);
        assert_eq!(t.level_color(Level::Warn), t.level_warn);
        assert_eq!(t.level_color(Level::Error), t.level_error);
        assert_eq!(t.level_color(Level::Fatal), t.level_fatal);
    }

    #[test]
    fn default_equals_dark() {
        // `Theme::default()` is just a sugared `dark()` — keep them aligned.
        let a = Theme::default();
        let b = Theme::dark();
        assert_eq!(a.accent, b.accent);
        assert_eq!(a.level_info, b.level_info);
    }
}
```

- [ ] **Step 2: Register in `mod.rs`**

Edit `crates/sift-cli/src/tui/mod.rs` — add `mod theme;` to the module declarations. Keep alphabetical:

```rust
mod input;
mod jsonfmt;
mod state;
mod theme;
mod view;
mod view_detail;
mod view_help;
mod view_search;
mod view_tail;
mod ws;
```

- [ ] **Step 3: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli tui::theme`
Expected: 3 new tests pass.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/tui/theme.rs crates/sift-cli/src/tui/mod.rs
git commit -m "feat(tui): theme.rs — single-file colour palette + dark default"
```

---

## Task 4: `focus.rs` — Pane enum + cycle logic

**Goal:** First-class focus state that views read to pick border styles, and that input dispatch uses to route text entry.

**Files:**
- Create: `crates/sift-cli/src/tui/focus.rs`
- Modify: `crates/sift-cli/src/tui/mod.rs` (register module)

- [ ] **Step 1: Create the module with failing tests**

Create `crates/sift-cli/src/tui/focus.rs`:

```rust
//! Pane focus — which bordered region is currently receiving input.

use super::state::Mode;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Pane {
    // Tail-mode panes
    Records,
    Filter,
    LevelsBar,
    RateBar, // non-focusable; excluded from cycle

    // Search-mode panes
    SearchQuery,
    SearchFrom,
    SearchTo,
    SearchResults,

    // Overlays (locked while open)
    DetailPopup,
    HelpPopup,
    CommandPrompt,
}

/// Order used for Tab/Shift+Tab cycling. RateBar is display-only — skipped.
const TAIL_CYCLE: &[Pane] = &[Pane::Records, Pane::Filter, Pane::LevelsBar];
const SEARCH_CYCLE: &[Pane] = &[
    Pane::SearchQuery,
    Pane::SearchFrom,
    Pane::SearchTo,
    Pane::SearchResults,
    Pane::LevelsBar,
];

#[must_use]
pub fn cycle(current: Pane, mode: Mode, forward: bool) -> Pane {
    // Overlays lock focus — cycling is a no-op.
    if matches!(
        current,
        Pane::DetailPopup | Pane::HelpPopup | Pane::CommandPrompt
    ) {
        return current;
    }

    let ring: &[Pane] = match mode {
        Mode::Tail => TAIL_CYCLE,
        Mode::Search => SEARCH_CYCLE,
    };
    let Some(idx) = ring.iter().position(|p| *p == current) else {
        // Current pane isn't in this mode's cycle (e.g. user just swapped
        // modes). Snap to the first entry of the new cycle.
        return ring[0];
    };
    let next = if forward {
        (idx + 1) % ring.len()
    } else {
        (idx + ring.len() - 1) % ring.len()
    };
    ring[next]
}

/// Default pane when a mode is first entered.
#[must_use]
pub fn default_pane(mode: Mode) -> Pane {
    match mode {
        Mode::Tail => Pane::Records,
        Mode::Search => Pane::SearchQuery,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_cycle_forward_hits_every_non_rate_pane() {
        let seq: Vec<Pane> = std::iter::successors(Some(Pane::Records), |p| {
            Some(cycle(*p, Mode::Tail, true))
        })
        .take(4)
        .collect();
        // Records → Filter → LevelsBar → Records
        assert_eq!(
            seq,
            vec![Pane::Records, Pane::Filter, Pane::LevelsBar, Pane::Records]
        );
    }

    #[test]
    fn tail_cycle_backward_is_reverse() {
        let seq: Vec<Pane> = std::iter::successors(Some(Pane::Records), |p| {
            Some(cycle(*p, Mode::Tail, false))
        })
        .take(4)
        .collect();
        assert_eq!(
            seq,
            vec![Pane::Records, Pane::LevelsBar, Pane::Filter, Pane::Records]
        );
    }

    #[test]
    fn search_cycle_includes_all_five_panes() {
        let seq: Vec<Pane> = std::iter::successors(Some(Pane::SearchQuery), |p| {
            Some(cycle(*p, Mode::Search, true))
        })
        .take(6)
        .collect();
        assert_eq!(
            seq,
            vec![
                Pane::SearchQuery,
                Pane::SearchFrom,
                Pane::SearchTo,
                Pane::SearchResults,
                Pane::LevelsBar,
                Pane::SearchQuery,
            ]
        );
    }

    #[test]
    fn overlay_focus_does_not_cycle() {
        assert_eq!(cycle(Pane::DetailPopup, Mode::Tail, true), Pane::DetailPopup);
        assert_eq!(cycle(Pane::HelpPopup, Mode::Tail, true), Pane::HelpPopup);
        assert_eq!(
            cycle(Pane::CommandPrompt, Mode::Search, false),
            Pane::CommandPrompt
        );
    }

    #[test]
    fn cycle_from_wrong_mode_snaps_to_first() {
        // A SearchQuery while in Tail mode should jump to Records.
        assert_eq!(cycle(Pane::SearchQuery, Mode::Tail, true), Pane::Records);
    }

    #[test]
    fn default_pane_per_mode() {
        assert_eq!(default_pane(Mode::Tail), Pane::Records);
        assert_eq!(default_pane(Mode::Search), Pane::SearchQuery);
    }
}
```

- [ ] **Step 2: Register in `mod.rs`**

```rust
mod focus;
mod input;
mod jsonfmt;
mod state;
mod theme;
mod view;
mod view_detail;
mod view_help;
mod view_search;
mod view_tail;
mod ws;
```

- [ ] **Step 3: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli tui::focus`
Expected: 6 new tests pass.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/tui/focus.rs crates/sift-cli/src/tui/mod.rs
git commit -m "feat(tui): focus.rs — Pane enum + Tab/Shift+Tab cycle logic"
```

---

## Task 5: `state.rs` extensions — theme, focus_pane, timestamp_mode, rate ring, flash

**Goal:** Plumb the new state fields into `AppState` so later view tasks can render from them. TDD each new field/method.

**Files:**
- Modify: `crates/sift-cli/src/tui/state.rs`

- [ ] **Step 1: Write failing tests in `state.rs` `mod tests`**

Append to the existing `mod tests` block:

```rust
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
        let past = Instant::now() - Duration::from_secs(5);
        s.flash = Some(Flash {
            msg: "old".into(),
            kind: FlashKind::Ok,
            show_until: past,
        });
        s.expire_flash_if_due();
        assert!(s.flash.is_none());
    }
```

- [ ] **Step 2: Run — verify fail**

Run: `cargo test -p sift-cli tui::state::tests::new_state_has_theme_focus_timestamp_defaults`
Expected: FAIL — undefined types/fields.

- [ ] **Step 3: Add the new types to `state.rs`**

Near the other enums in `state.rs`:

```rust
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
```

- [ ] **Step 4: Extend `AppState`**

Add fields to `AppState` struct. Use `pub(super)` for private-ish plumbing, keep `pub` for anything views read:

```rust
pub struct AppState {
    // existing…
    pub theme: crate::tui::theme::Theme,
    pub focus_pane: crate::tui::focus::Pane,
    pub timestamp_mode: TimestampMode,

    pub rate_history: std::collections::VecDeque<u32>,
    pub rate_current: u32,
    pub(super) rate_second_start: std::time::Instant,

    pub flash: Option<Flash>,
}
```

The `rate_second_start: Instant` can't be `Default`ed straightforwardly. Constructing it in `new()` is fine.

- [ ] **Step 5: Initialise in `AppState::new()`**

In `AppState::new()` add:

```rust
            theme: crate::tui::theme::Theme::dark(),
            focus_pane: crate::tui::focus::Pane::Records,
            timestamp_mode: TimestampMode::Absolute,

            rate_history: std::collections::VecDeque::with_capacity(60),
            rate_current: 0,
            rate_second_start: std::time::Instant::now(),

            flash: None,
```

- [ ] **Step 6: Add the new methods**

Place next to other `AppState` methods:

```rust
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
```

- [ ] **Step 7: Run tests — verify pass**

Run: `cargo fmt --all && cargo test -p sift-cli tui::state`
Expected: 5 new tests pass, existing state tests unchanged.

- [ ] **Step 8: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add crates/sift-cli/src/tui/state.rs
git commit -m "feat(tui): state extensions — theme, focus, timestamp mode, rate ring, flash"
```

---

## Task 6: `command.rs` — parse + complete the `:` palette

**Goal:** Pure parser for the command-mode grammar. No state integration yet — this task is the grammar, fully tested.

**Files:**
- Create: `crates/sift-cli/src/tui/command.rs`
- Modify: `crates/sift-cli/src/tui/mod.rs` (register module)

- [ ] **Step 1: Create `command.rs` with tests and impl**

Create `crates/sift-cli/src/tui/command.rs`:

```rust
//! `:` command palette — parser + completion.
//!
//! Grammar (EBNF-ish):
//!   command  ::= ':' verb (SP kv)*
//!   verb     ::= 'pause' | 'resume' | 'clear' | 'retry' | 'save'
//!              | 'search' | 'level' | 'help' | 'quit'
//!   kv       ::= key '=' value        | bare_arg (for `:level`)
//!   key      ::= [a-z][a-z0-9_]*
//!   value    ::= '"' ... '"' | [^ "=]+

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use sift_core::Level;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandAction {
    Pause,
    Resume,
    Clear,
    Retry,
    Save { path: PathBuf },
    Search {
        q: Option<String>,
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    },
    Level { level: Level, on: bool },
    Help,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    Empty,
    UnknownVerb(String),
    UnknownKey { verb: &'static str, key: String },
    MissingKey { verb: &'static str, key: &'static str },
    BadValue { key: &'static str, reason: String },
    BadTime { key: &'static str, value: String },
    BadLevel(String),
    BadToggle(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty command"),
            Self::UnknownVerb(v) => write!(f, "unknown verb \"{v}\""),
            Self::UnknownKey { verb, key } => write!(f, ":{verb} takes no `{key}` key"),
            Self::MissingKey { verb, key } => write!(f, ":{verb} requires `{key}`"),
            Self::BadValue { key, reason } => write!(f, "{key}: {reason}"),
            Self::BadTime { key, value } => write!(f, "{key}=\"{value}\" is not RFC3339"),
            Self::BadLevel(s) => write!(f, "unknown level \"{s}\""),
            Self::BadToggle(s) => write!(f, "expected `on` or `off`, got \"{s}\""),
        }
    }
}

impl std::error::Error for CommandError {}

const ALL_VERBS: &[&str] = &[
    "pause", "resume", "clear", "retry", "save", "search", "level", "help", "quit",
];

/// Parse a user-typed command string. Input may or may not start with `:` — we
/// accept both. Returns Err on any grammar or type problem.
pub fn parse(input: &str) -> Result<CommandAction, CommandError> {
    let input = input.trim_start_matches(':').trim();
    if input.is_empty() {
        return Err(CommandError::Empty);
    }

    let mut iter = input.split_whitespace();
    let verb = iter.next().ok_or(CommandError::Empty)?;
    let rest: Vec<&str> = iter.collect();

    match verb {
        "pause" => Ok(CommandAction::Pause),
        "resume" => Ok(CommandAction::Resume),
        "clear" => Ok(CommandAction::Clear),
        "retry" => Ok(CommandAction::Retry),
        "help" => Ok(CommandAction::Help),
        "quit" => Ok(CommandAction::Quit),
        "save" => parse_save(&rest),
        "search" => parse_search(&rest),
        "level" => parse_level(&rest),
        other => Err(CommandError::UnknownVerb(other.into())),
    }
}

fn parse_save(args: &[&str]) -> Result<CommandAction, CommandError> {
    let mut path: Option<PathBuf> = None;
    for a in args {
        match split_kv(a) {
            Some(("path", v)) => path = Some(PathBuf::from(v)),
            Some((k, _)) => {
                return Err(CommandError::UnknownKey {
                    verb: "save",
                    key: k.into(),
                })
            }
            None => {
                return Err(CommandError::BadValue {
                    key: "save",
                    reason: format!("expected key=value, got \"{a}\""),
                })
            }
        }
    }
    let path = path.ok_or(CommandError::MissingKey {
        verb: "save",
        key: "path",
    })?;
    Ok(CommandAction::Save { path })
}

fn parse_search(args: &[&str]) -> Result<CommandAction, CommandError> {
    let mut q: Option<String> = None;
    let mut from: Option<DateTime<Utc>> = None;
    let mut to: Option<DateTime<Utc>> = None;
    for a in args {
        match split_kv(a) {
            Some(("q", v)) => q = Some(unquote(v).to_string()),
            Some(("from", v)) => {
                from = Some(parse_rfc3339(v, "from")?);
            }
            Some(("to", v)) => {
                to = Some(parse_rfc3339(v, "to")?);
            }
            Some((k, _)) => {
                return Err(CommandError::UnknownKey {
                    verb: "search",
                    key: k.into(),
                })
            }
            None => {
                return Err(CommandError::BadValue {
                    key: "search",
                    reason: format!("expected key=value, got \"{a}\""),
                })
            }
        }
    }
    Ok(CommandAction::Search { q, from, to })
}

fn parse_level(args: &[&str]) -> Result<CommandAction, CommandError> {
    if args.len() != 2 {
        return Err(CommandError::BadValue {
            key: "level",
            reason: "usage: :level <name> on|off".into(),
        });
    }
    let level = parse_level_name(args[0])?;
    let on = match args[1] {
        "on" | "true" => true,
        "off" | "false" => false,
        other => return Err(CommandError::BadToggle(other.into())),
    };
    Ok(CommandAction::Level { level, on })
}

fn parse_level_name(s: &str) -> Result<Level, CommandError> {
    match s.to_ascii_lowercase().as_str() {
        "trace" => Ok(Level::Trace),
        "debug" => Ok(Level::Debug),
        "info" => Ok(Level::Info),
        "warn" => Ok(Level::Warn),
        "error" => Ok(Level::Error),
        "fatal" => Ok(Level::Fatal),
        _ => Err(CommandError::BadLevel(s.into())),
    }
}

fn parse_rfc3339(v: &str, key: &'static str) -> Result<DateTime<Utc>, CommandError> {
    DateTime::parse_from_rfc3339(unquote(v))
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| CommandError::BadTime {
            key,
            value: v.into(),
        })
}

fn split_kv(s: &str) -> Option<(&str, &str)> {
    s.split_once('=')
}

fn unquote(s: &str) -> &str {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Tab-completion result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Longest shared prefix of all matching verbs (may equal the input itself
    /// when no unique completion exists).
    pub prefix: String,
    /// All matching verbs (for display — unused by the MVP UI).
    pub matches: Vec<&'static str>,
}

/// Tab-complete the verb portion of a command. `input` may or may not start
/// with `:`. Only completes the first word; does nothing useful if the input
/// already has whitespace.
#[must_use]
pub fn complete(input: &str) -> Completion {
    let stripped = input.trim_start_matches(':');
    let partial = stripped.split_whitespace().next().unwrap_or("");
    let matches: Vec<&'static str> = ALL_VERBS
        .iter()
        .copied()
        .filter(|v| v.starts_with(partial))
        .collect();
    let prefix = longest_shared_prefix(&matches).to_string();
    Completion { prefix, matches }
}

fn longest_shared_prefix(words: &[&'static str]) -> &'static str {
    match words {
        [] => "",
        [only] => only,
        [first, rest @ ..] => {
            let mut upper = first.len();
            for w in rest {
                let mut common = 0;
                for (a, b) in first.as_bytes().iter().zip(w.as_bytes().iter()) {
                    if a == b {
                        common += 1;
                    } else {
                        break;
                    }
                }
                upper = upper.min(common);
            }
            &first[..upper]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_verbs_returns_expected_action() {
        assert_eq!(parse(":pause").unwrap(), CommandAction::Pause);
        assert_eq!(parse(":resume").unwrap(), CommandAction::Resume);
        assert_eq!(parse(":clear").unwrap(), CommandAction::Clear);
        assert_eq!(parse(":retry").unwrap(), CommandAction::Retry);
        assert_eq!(parse(":help").unwrap(), CommandAction::Help);
        assert_eq!(parse(":quit").unwrap(), CommandAction::Quit);
    }

    #[test]
    fn colon_prefix_is_optional() {
        assert_eq!(parse("pause").unwrap(), CommandAction::Pause);
    }

    #[test]
    fn empty_is_err_empty() {
        assert_eq!(parse("").unwrap_err(), CommandError::Empty);
        assert_eq!(parse(":").unwrap_err(), CommandError::Empty);
        assert_eq!(parse("   ").unwrap_err(), CommandError::Empty);
    }

    #[test]
    fn unknown_verb_reports_name() {
        match parse(":frobnicate").unwrap_err() {
            CommandError::UnknownVerb(v) => assert_eq!(v, "frobnicate"),
            other => panic!("expected UnknownVerb, got {other:?}"),
        }
    }

    #[test]
    fn save_requires_path() {
        assert!(matches!(
            parse(":save").unwrap_err(),
            CommandError::MissingKey { verb: "save", key: "path" }
        ));
    }

    #[test]
    fn save_accepts_path() {
        let got = parse(":save path=foo.ndjson").unwrap();
        assert_eq!(
            got,
            CommandAction::Save {
                path: PathBuf::from("foo.ndjson")
            }
        );
    }

    #[test]
    fn save_rejects_unknown_key() {
        match parse(":save format=csv").unwrap_err() {
            CommandError::UnknownKey { verb, key } => {
                assert_eq!(verb, "save");
                assert_eq!(key, "format");
            }
            other => panic!("expected UnknownKey, got {other:?}"),
        }
    }

    #[test]
    fn search_accepts_q_from_to() {
        let got = parse(":search q=error from=2026-04-23T00:00:00Z to=2026-04-23T01:00:00Z")
            .unwrap();
        match got {
            CommandAction::Search { q, from, to } => {
                assert_eq!(q.as_deref(), Some("error"));
                assert!(from.is_some());
                assert!(to.is_some());
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn search_q_may_be_quoted() {
        let got = parse(":search q=\"hello world\"").unwrap();
        match got {
            CommandAction::Search { q, .. } => assert_eq!(q.as_deref(), Some("hello world")),
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn search_rejects_bad_time() {
        assert!(matches!(
            parse(":search from=not-rfc3339").unwrap_err(),
            CommandError::BadTime { key: "from", .. }
        ));
    }

    #[test]
    fn level_requires_name_and_toggle() {
        let got = parse(":level warn on").unwrap();
        assert_eq!(got, CommandAction::Level { level: Level::Warn, on: true });
    }

    #[test]
    fn level_case_insensitive() {
        let got = parse(":level WARN off").unwrap();
        assert_eq!(got, CommandAction::Level { level: Level::Warn, on: false });
    }

    #[test]
    fn level_rejects_bad_name() {
        match parse(":level banana on").unwrap_err() {
            CommandError::BadLevel(s) => assert_eq!(s, "banana"),
            other => panic!("expected BadLevel, got {other:?}"),
        }
    }

    #[test]
    fn level_rejects_bad_toggle() {
        match parse(":level warn maybe").unwrap_err() {
            CommandError::BadToggle(s) => assert_eq!(s, "maybe"),
            other => panic!("expected BadToggle, got {other:?}"),
        }
    }

    #[test]
    fn complete_p_returns_pause() {
        let c = complete(":p");
        assert!(c.matches.contains(&"pause"));
        assert_eq!(c.prefix, "pause");
    }

    #[test]
    fn complete_re_returns_resume_and_retry() {
        let c = complete(":re");
        assert!(c.matches.contains(&"resume"));
        assert!(c.matches.contains(&"retry"));
        // Longest shared prefix of "resume" and "retry" is "re".
        assert_eq!(c.prefix, "re");
    }

    #[test]
    fn complete_empty_returns_all_verbs() {
        let c = complete("");
        assert_eq!(c.matches.len(), ALL_VERBS.len());
        // Longest shared prefix of all 9 verbs is "".
        assert_eq!(c.prefix, "");
    }

    #[test]
    fn complete_no_match_returns_empty_list() {
        let c = complete(":zzz");
        assert!(c.matches.is_empty());
        assert_eq!(c.prefix, "");
    }
}
```

- [ ] **Step 2: Register in `mod.rs`**

```rust
mod command;
mod focus;
mod input;
mod jsonfmt;
mod state;
mod theme;
mod view;
mod view_detail;
mod view_help;
mod view_search;
mod view_tail;
mod ws;
```

- [ ] **Step 3: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli tui::command`
Expected: 18 new tests pass.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/tui/command.rs crates/sift-cli/src/tui/mod.rs
git commit -m "feat(tui): command.rs — `:` palette parser + tab-complete"
```

---

## Task 7: Command mode — state + input dispatcher integration

**Goal:** `:` in Normal mode opens a command prompt; typed characters go into `command_input`; Enter parses + (for now) stashes the resulting `CommandAction` in a new `pending_command` field for Task 18 to dispatch; Esc cancels; Tab tab-completes the prefix.

**Files:**
- Modify: `crates/sift-cli/src/tui/state.rs`
- Modify: `crates/sift-cli/src/tui/input.rs`

- [ ] **Step 1: Failing tests in `input.rs` `mod tests`**

Append:

```rust
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
        assert_eq!(s.filter, ":");
    }
```

- [ ] **Step 2: Run — verify fail**

Run: `cargo test -p sift-cli tui::input::tests::colon_in_tail_list_opens_command_prompt`
Expected: FAIL (command_open undefined, etc.).

- [ ] **Step 3: Add state fields**

Edit `crates/sift-cli/src/tui/state.rs`. Import `tui_input::Input` at the top:

```rust
use tui_input::Input;
```

Add fields to `AppState`:

```rust
    pub command_open: bool,
    pub command_input: Input,
    pub pending_command: Option<crate::tui::command::CommandAction>,
```

Initialise in `new()`:

```rust
            command_open: false,
            command_input: Input::default(),
            pending_command: None,
```

Add helper:

```rust
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
```

- [ ] **Step 4: Update `input.rs::dispatch`**

Edit `crates/sift-cli/src/tui/input.rs`. At the top, add imports:

```rust
use tui_input::backend::crossterm::EventHandler;
```

Replace `dispatch`:

```rust
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

    if state.help_open {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') => state.toggle_help(),
            _ => {}
        }
        return;
    }

    if state.detail.is_some() {
        if key.code == KeyCode::Esc {
            state.close_detail();
        }
        return;
    }

    let editing_text = matches!(
        (state.mode, state.focus),
        (Mode::Tail, Focus::Filter) | (Mode::Search, _)
    );

    // `?` toggles help outside text-editing contexts.
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
    if key.code == KeyCode::Char('r')
        && state.mode == Mode::Tail
        && state.focus == Focus::List
    {
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
```

- [ ] **Step 5: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli tui::input`
Expected: all new + existing input tests pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/src/tui/state.rs crates/sift-cli/src/tui/input.rs
git commit -m "feat(tui): command prompt wiring — `:` opens, Enter parses, Tab completes"
```

---

## Task 8: `view_footer.rs` — mode badge + hints + command prompt + flash

**Goal:** One-row footer that renders four states cleanly: Normal (keybind hints + `[N]`), Insert (same hints dimmer + `[I]`), Command (`: …` prompt + `[C]`), and Flash (flash message replaces hints for ~2s).

**Files:**
- Create: `crates/sift-cli/src/tui/view_footer.rs`
- Modify: `crates/sift-cli/src/tui/view.rs` (register + wire into top-level draw)
- Modify: `crates/sift-cli/src/tui/mod.rs` (register module)

- [ ] **Step 1: Create `view_footer.rs`**

```rust
//! One-row footer. Renders either keybind hints, the `:` command prompt,
//! or a transient flash message — plus a right-aligned mode badge.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::focus::Pane;
use super::state::{AppState, FlashKind, Mode};

pub fn draw_footer(f: &mut Frame, area: Rect, state: &AppState) {
    let [left, right] = Layout::horizontal([Constraint::Min(1), Constraint::Length(5)]).areas(area);

    render_left(f, left, state);
    render_mode_badge(f, right, state);
}

fn render_left(f: &mut Frame, area: Rect, state: &AppState) {
    if state.command_open {
        let line = Line::from(vec![
            Span::styled(
                " : ",
                Style::default().fg(state.theme.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(state.command_input.value().to_string()),
        ]);
        f.render_widget(Paragraph::new(line), area);
        // Position the terminal cursor at the input's visible position.
        let x = area.x + 3 + u16::try_from(state.command_input.visual_cursor()).unwrap_or(0);
        f.set_cursor_position((x, area.y));
        return;
    }

    if let Some(flash) = &state.flash {
        let fg = match flash.kind {
            FlashKind::Ok => state.theme.flash_ok,
            FlashKind::Err => state.theme.flash_err,
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {} ", flash.msg),
                Style::default().fg(fg).add_modifier(Modifier::BOLD),
            )),
            area,
        );
        return;
    }

    f.render_widget(Paragraph::new(hints_for(state)), area);
}

fn hints_for(state: &AppState) -> Line<'static> {
    let key = Style::default().fg(state.theme.shortcut_key);
    let lbl = Style::default().fg(state.theme.shortcut_label);
    let sep = Span::styled(" · ", Style::default().fg(state.theme.muted));

    let hints: Vec<(&'static str, &'static str)> = match state.mode {
        Mode::Tail => vec![
            ("/", "filter"),
            ("Space", "pause"),
            ("↑↓", "nav"),
            ("↵", "detail"),
            ("s", "search"),
            (":", "cmd"),
            ("?", "help"),
            ("q", "quit"),
        ],
        Mode::Search => vec![
            ("Tab", "next"),
            ("↵", "search"),
            ("o", "detail"),
            ("t", "tail"),
            (":", "cmd"),
            ("?", "help"),
        ],
    };

    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    for (i, (k, l)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(sep.clone());
        }
        spans.push(Span::styled((*k).to_string(), key));
        spans.push(Span::raw(" "));
        spans.push(Span::styled((*l).to_string(), lbl));
    }
    Line::from(spans)
}

fn render_mode_badge(f: &mut Frame, area: Rect, state: &AppState) {
    let badge = if state.command_open {
        "[C]"
    } else if is_insert_mode(state) {
        "[I]"
    } else {
        "[N]"
    };
    f.render_widget(
        Paragraph::new(Span::styled(
            badge,
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(ratatui::layout::Alignment::Right),
        area,
    );
}

fn is_insert_mode(state: &AppState) -> bool {
    matches!(
        state.focus_pane,
        Pane::Filter | Pane::SearchQuery | Pane::SearchFrom | Pane::SearchTo
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_into_buffer(state: &AppState) -> String {
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw_footer(f, f.area(), state))
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect::<String>()
    }

    #[test]
    fn normal_mode_shows_hints_and_n_badge() {
        let s = AppState::new();
        let out = render_into_buffer(&s);
        assert!(out.contains("filter"));
        assert!(out.contains("[N]"));
    }

    #[test]
    fn command_mode_shows_colon_prompt_and_c_badge() {
        let mut s = AppState::new();
        s.open_command_prompt();
        s.command_input = tui_input::Input::new("pa".into());
        let out = render_into_buffer(&s);
        assert!(out.contains(":"));
        assert!(out.contains("pa"));
        assert!(out.contains("[C]"));
    }

    #[test]
    fn flash_err_shows_message_instead_of_hints() {
        let mut s = AppState::new();
        s.set_flash_err("boom");
        let out = render_into_buffer(&s);
        assert!(out.contains("boom"));
        assert!(!out.contains("filter"));
    }

    #[test]
    fn insert_mode_shows_i_badge() {
        let mut s = AppState::new();
        s.focus_pane = Pane::Filter;
        let out = render_into_buffer(&s);
        assert!(out.contains("[I]"));
    }
}
```

- [ ] **Step 2: Register in `mod.rs`**

```rust
mod command;
mod focus;
mod input;
mod jsonfmt;
mod state;
mod theme;
mod view;
mod view_detail;
mod view_footer;
mod view_help;
mod view_search;
mod view_tail;
mod ws;
```

- [ ] **Step 3: Wire into `view.rs`**

Replace `crates/sift-cli/src/tui/view.rs`:

```rust
//! Top-level draw dispatcher. Picks a mode view and overlays detail/help/footer.

use ratatui::layout::{Constraint, Layout};
use ratatui::Frame;

use super::state::{AppState, Mode};
use super::view_detail::draw_detail;
use super::view_footer::draw_footer;
use super::view_help::draw_help;
use super::view_search::draw_search;
use super::view_tail::draw_tail;

pub fn draw(f: &mut Frame, state: &AppState) {
    let [body, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

    match state.mode {
        Mode::Tail => draw_tail(f, body, state),
        Mode::Search => draw_search(f, body, state),
    }
    draw_footer(f, footer, state);

    if let Some(r) = &state.detail {
        draw_detail(f, r);
    }
    if state.help_open {
        draw_help(f);
    }
}
```

This adds an `area: Rect` parameter to `draw_tail` and `draw_search`. Subsequent tasks rewrite those functions anyway — but this task makes them take `area` too. Patch both NOW:

Edit `view_tail.rs::draw_tail` signature from `pub fn draw_tail(f: &mut Frame, state: &AppState)` to `pub fn draw_tail(f: &mut Frame, area: Rect, state: &AppState)` and internally use `area` instead of `f.area()`. Same for `view_search.rs::draw_search`. (Task 10 will replace these entirely; this is a minimal compile-fix patch.)

- [ ] **Step 4: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass, including 4 new `view_footer` tests.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-cli/src/tui/
git commit -m "feat(tui): view_footer — mode badge + hints + command prompt + flash"
```

---

## Task 9: `record_input.rs` + filter migrates to tui-input

**Goal:** Replace the hand-rolled `state.filter: String` + push-char/pop-char logic with `tui-input::Input` so the filter bar has real cursor handling (Home/End, arrow keys, word-jump, etc.). Thin wrapper module to normalize input interactions across all four text fields.

**Files:**
- Create: `crates/sift-cli/src/tui/record_input.rs`
- Modify: `crates/sift-cli/src/tui/state.rs` (filter becomes `Input`)
- Modify: `crates/sift-cli/src/tui/input.rs` (filter focus uses tui-input)

- [ ] **Step 1: Create `record_input.rs`**

```rust
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
```

- [ ] **Step 2: Register in `mod.rs`**

```rust
mod command;
mod focus;
mod input;
mod jsonfmt;
mod record_input;
mod state;
mod theme;
mod view;
mod view_detail;
mod view_footer;
mod view_help;
mod view_search;
mod view_tail;
mod ws;
```

- [ ] **Step 3: Migrate `state.filter` from `String` to `Input`**

Edit `crates/sift-cli/src/tui/state.rs`:

Change the field:

```rust
    pub filter: tui_input::Input,
```

Init in `new()`:

```rust
            filter: tui_input::Input::default(),
```

Update `set_filter`:

```rust
    pub fn set_filter(&mut self, s: String) {
        self.filter = tui_input::Input::new(s);
        self.scroll = ScrollMode::Follow;
    }
```

Update every reader of `state.filter` in the codebase. Grep:

```bash
grep -rn "state.filter\b\|self.filter\b" crates/sift-cli/src/tui/
```

The readers (`state.rs::filtered`, `view_tail::render_filter`, etc.) need to be changed from `&self.filter` (was `&String`) to `self.filter.value()` (is `&str`).

Also the `set_filter` test needs updating:

```rust
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
```

still works since `set_filter` takes `String`.

And:

```rust
    #[test]
    fn filter_substring_filters_body() {
        let mut s = AppState::new();
        s.push_record(rec("hello world"));
        s.push_record(rec("goodbye"));
        s.set_filter("hello".into());
        assert_eq!(s.filtered().len(), 1);
        assert_eq!(s.filtered()[0].body, "hello world");
    }
```

— still works.

`filtered()` now reads `self.filter.value()`:

```rust
    #[must_use]
    pub fn filtered(&self) -> Vec<&Record> {
        let needle = self.filter.value();
        self.records
            .iter()
            .filter(|r| self.level_set.allows(r.level))
            .filter(|r| needle.is_empty() || r.body.contains(needle))
            .collect()
    }
```

- [ ] **Step 4: Update `input.rs::tail_filter`**

Replace the filter handler to route through `record_input::feed`:

```rust
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
```

(The scroll-reset side effect used to live in `set_filter`; it's preserved here.)

- [ ] **Step 5: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all tests pass. The filter substring test in state.rs should still work (because `set_filter("hello".into())` still produces a filter that matches `"hello world"`).

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/src/tui/
git commit -m "feat(tui): migrate filter to tui-input — cursor keys, home/end, word nav"
```

---

## Task 10: `view_tail.rs` rewrite — outer rounded Block + Records Table

**Goal:** Replace the hand-rolled List + space-padded strings with a proper `Table` widget inside a rounded outer `Block`. Title carries the tab strip and the `N/M · N/s` meta. Level column is a colored block glyph, selected row shows `▕` gutter. Tri-section middle bar goes in next task.

**Files:**
- Modify: `crates/sift-cli/src/tui/view_tail.rs`

- [ ] **Step 1: Rewrite `view_tail.rs`**

Replace the file:

```rust
//! Tail view — outer rounded Block with tab-strip title, Records Table,
//! tri-section middle bar (filter/levels/rate). Footer is drawn by view_footer.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Title;
use ratatui::widgets::{Block, BorderType, Borders, Cell, Row, Table};
use ratatui::Frame;
use sift_core::{Level, Record};

use super::focus::Pane;
use super::state::{AppState, Mode};

pub fn draw_tail(f: &mut Frame, area: Rect, state: &AppState) {
    let outer = outer_block(state);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Split into records (fills) and middle bar (3 rows, including borders).
    let [records_area, middle_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(inner);

    render_records(f, records_area, state);
    super::view_tail_middle::draw_middle(f, middle_area, state);
}

fn outer_block(state: &AppState) -> Block<'static> {
    let tabs = Line::from(vec![
        Span::raw(" sift  •  "),
        Span::styled(
            "⟨ Tail ⟩",
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" · "),
        Span::styled(
            "Search",
            Style::default().fg(state.theme.muted),
        ),
        Span::raw(" "),
    ]);

    let count = state.filtered().len();
    let cap = crate::tui::state::RECORD_CAP;
    let rate = state.rate_per_sec();
    let meta = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            format!("{count}/{cap}"),
            Style::default().fg(state.theme.muted),
        ),
        Span::styled("  •  ", Style::default().fg(state.theme.muted)),
        Span::styled(
            format!("{rate}/s"),
            Style::default().fg(state.theme.muted),
        ),
        Span::raw(" "),
    ]);

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style(state, Pane::Records))
        .title(tabs)
        .title(Title::from(meta).alignment(Alignment::Right))
}

fn border_style(state: &AppState, pane: Pane) -> Style {
    if state.focus_pane == pane {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    }
}

fn render_records(f: &mut Frame, area: Rect, state: &AppState) {
    let recs = state.filtered();
    if recs.is_empty() {
        let msg = if state.filter.value().is_empty() {
            "waiting for records…"
        } else {
            "no records match the filter"
        };
        f.render_widget(
            ratatui::widgets::Paragraph::new(msg)
                .style(Style::default().fg(state.theme.muted)),
            area,
        );
        return;
    }

    let visible = area.height as usize;
    let cursor = state.tail_cursor_or_last();
    let (start, end) = window_around_cursor(recs.len(), visible.saturating_sub(1), cursor);

    let header = Row::new(vec![
        Cell::from("time"),
        Cell::from("host"),
        Cell::from("app"),
        Cell::from("lvl"),
        Cell::from("body"),
    ])
    .style(Style::default().fg(state.theme.muted))
    .bottom_margin(0);

    let rows: Vec<Row> = recs[start..end]
        .iter()
        .enumerate()
        .map(|(i, r)| row(r, start + i == cursor, state))
        .collect();

    let widths = [
        Constraint::Length(12),
        Constraint::Length(10),
        Constraint::Length(7),
        Constraint::Length(5),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(if state.focus_pane == Pane::Records {
                    state.theme.bg_selected
                } else {
                    state.theme.bg_selected_unfocused
                })
                .fg(state.theme.fg_selected),
        )
        .highlight_symbol("▕ ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    f.render_widget(table, area);
}

fn row<'a>(r: &'a Record, selected: bool, state: &AppState) -> Row<'a> {
    let ts = r.timestamp.format("%H:%M:%S%.3f").to_string();
    let level_cell = level_block_cell(r.level, state);
    let body = r.body.clone();
    let cells = vec![
        Cell::from(ts).style(Style::default().fg(state.theme.muted)),
        Cell::from(r.host.clone()).style(Style::default().fg(state.theme.fg)),
        Cell::from(r.app.clone()).style(Style::default().fg(state.theme.fg)),
        level_cell,
        Cell::from(body).style(Style::default().fg(state.theme.fg)),
    ];
    let row = Row::new(cells);
    if selected {
        row.style(Style::default().add_modifier(Modifier::BOLD))
    } else {
        row
    }
}

fn level_block_cell(lvl: Level, state: &AppState) -> Cell<'static> {
    let color = state.theme.level_color(lvl);
    let text = match lvl {
        Level::Trace => "░░░",
        Level::Debug => "▒▒▒",
        _ => "███",
    };
    Cell::from(text).style(Style::default().fg(color))
}

/// Compute the (start, end) slice of `recs` that should be visible given
/// `visible_rows` and the cursor index. Keeps the cursor on screen.
pub(super) fn window_around_cursor(
    total: usize,
    visible_rows: usize,
    cursor: usize,
) -> (usize, usize) {
    if total == 0 || visible_rows == 0 {
        return (0, 0);
    }
    let visible = visible_rows.min(total);
    let cursor = cursor.min(total.saturating_sub(1));
    let start = if cursor + 1 >= visible {
        cursor + 1 - visible
    } else {
        0
    };
    let end = (start + visible).min(total);
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_scrolls_to_keep_cursor_visible() {
        // 100 total, 10 visible, cursor at 50 — window is 41..51.
        assert_eq!(window_around_cursor(100, 10, 50), (41, 51));
    }

    #[test]
    fn window_clamps_at_top() {
        assert_eq!(window_around_cursor(100, 10, 0), (0, 10));
    }

    #[test]
    fn window_clamps_at_end_when_cursor_past_total() {
        // 5 total, 10 visible requested — show all, cursor is clamped.
        assert_eq!(window_around_cursor(5, 10, 99), (0, 5));
    }

    #[test]
    fn window_empty_inputs_return_zero() {
        assert_eq!(window_around_cursor(0, 10, 0), (0, 0));
        assert_eq!(window_around_cursor(10, 0, 0), (0, 0));
    }
}
```

Note: this file references `super::view_tail_middle::draw_middle` — we create that file in Task 11. For this task's commit, either:
- (a) skip the middle-bar call and leave a temporary `Paragraph::new("")` render on `middle_area`; or
- (b) commit a stub `view_tail_middle.rs` with an empty `draw_middle(_, _, _)`.

Go with **(b)** — it keeps the diff clean and the next task substantive. Create `crates/sift-cli/src/tui/view_tail_middle.rs`:

```rust
//! Tail-mode middle bar (filter / levels / rate). Stub — Task 11 fills this in.

use ratatui::layout::Rect;
use ratatui::Frame;

use super::state::AppState;

pub fn draw_middle(_f: &mut Frame, _area: Rect, _state: &AppState) {
    // Intentionally blank until Task 11.
}
```

Register in `mod.rs`:

```rust
mod command;
mod focus;
mod input;
mod jsonfmt;
mod record_input;
mod state;
mod theme;
mod view;
mod view_detail;
mod view_footer;
mod view_help;
mod view_search;
mod view_tail;
mod view_tail_middle;
mod ws;
```

- [ ] **Step 2: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli tui::view_tail`
Expected: 4 `window_around_cursor` tests pass, pre-existing tests from old `visible_range` are removed (the function went away).

- [ ] **Step 3: Build + smoke**

Run: `cargo build -p sift-cli`
Expected: clean.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/tui/
git commit -m "feat(tui): view_tail — outer rounded Block + Records Table + level block glyphs"
```

---

## Task 11: Tri-section middle bar (filter / levels / rate blocks)

**Goal:** Fill in `view_tail_middle.rs` — three bordered `Block`s side by side carrying the filter input, levels row, and rate sparkline.

**Files:**
- Modify: `crates/sift-cli/src/tui/view_tail_middle.rs`

- [ ] **Step 1: Rewrite `view_tail_middle.rs`**

```rust
//! Tail-mode middle bar — three bordered blocks: filter / levels / rate.
//! Each block's border thickens + colours when that pane is focused.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Sparkline};
use ratatui::Frame;

use super::focus::Pane;
use super::state::AppState;

pub fn draw_middle(f: &mut Frame, area: Rect, state: &AppState) {
    let [filter_area, levels_area, rate_area] = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .areas(area);

    draw_filter(f, filter_area, state);
    draw_levels(f, levels_area, state);
    draw_rate(f, rate_area, state);
}

fn draw_filter(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus_pane == Pane::Filter;
    let block = section_block(" filter ", focused, state);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let marker = if focused { "▸ " } else { "  " };
    let line = Line::from(vec![
        Span::styled(marker, Style::default().fg(state.theme.accent)),
        Span::raw(state.filter.value().to_string()),
    ]);
    f.render_widget(Paragraph::new(line), inner);

    if focused {
        // Position cursor at the visual cursor position inside the input.
        let x = inner.x + 2 + u16::try_from(state.filter.visual_cursor()).unwrap_or(0);
        f.set_cursor_position((x, inner.y));
    }
}

fn draw_levels(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus_pane == Pane::LevelsBar;
    let block = section_block(" levels ", focused, state);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let set = state.level_set;
    let pills = [
        ('I', set.info, state.theme.level_info),
        ('W', set.warn, state.theme.level_warn),
        ('E', set.error, state.theme.level_error),
        ('F', set.fatal, state.theme.level_fatal),
        ('D', set.debug, state.theme.level_debug),
        ('T', set.trace, state.theme.level_trace),
    ];

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(pills.len() * 3);
    for (i, (letter, on, color)) in pills.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let dot = if *on { "●" } else { "○" };
        let letter_style = if *on {
            Style::default().fg(*color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(state.theme.disabled)
        };
        spans.push(Span::styled(
            dot.to_string(),
            Style::default().fg(if *on { *color } else { state.theme.disabled }),
        ));
        spans.push(Span::styled(letter.to_string(), letter_style));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn draw_rate(f: &mut Frame, area: Rect, state: &AppState) {
    // Rate is display-only; it never takes focus, but we still style the
    // border per the theme.
    let block = section_block(" rate 1m ", false, state);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [spark_area, number_area] =
        Layout::horizontal([Constraint::Min(5), Constraint::Length(7)]).areas(inner);

    let data: Vec<u64> = state.rate_history.iter().map(|x| u64::from(*x)).collect();
    let sparkline = Sparkline::default()
        .data(data)
        .style(Style::default().fg(state.theme.accent));
    f.render_widget(sparkline, spark_area);

    let rate = state.rate_per_sec();
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("{rate}/s"),
            Style::default().fg(state.theme.muted),
        )),
        number_area,
    );
}

fn section_block(title: &'static str, focused: bool, state: &AppState) -> Block<'static> {
    let border_style = if focused {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    };
    let border_type = if focused {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(
            title,
            Style::default().fg(state.theme.muted),
        ))
}
```

Note: Ratatui 0.30's `Sparkline::data` API may take `&[u64]` or `Vec<u64>`; if the borrow checker complains, adapt. The important property is "pipes a 60-sample Vec into the widget and renders block-glyph bars."

- [ ] **Step 2: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: pre-existing tests pass; this task has no new state tests (pure view).

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-cli/src/tui/view_tail_middle.rs
git commit -m "feat(tui): tri-section middle bar — filter/levels/rate blocks"
```

---

## Task 12: Sparkline feeder — 1 Hz rate tick in `mod.rs`

**Goal:** Drive `rate_current → rate_history` rotation and flash expiry from a periodic tokio tick in the main event loop. Rate sparkline becomes alive; flash messages auto-clear after 2s.

**Files:**
- Modify: `crates/sift-cli/src/tui/mod.rs`
- Modify: `crates/sift-cli/src/tui/state.rs` (tick new records into `record_rate_inc`)

- [ ] **Step 1: Tick WsEvent into the rate counter**

Edit `crates/sift-cli/src/tui/state.rs::apply_ws_event`:

```rust
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
```

- [ ] **Step 2: Add a 1 Hz tick arm to the main event loop**

Edit `crates/sift-cli/src/tui/mod.rs`. Where the redraw ticker lives, add a second interval at 1s cadence:

```rust
    let mut redraw = interval(Duration::from_millis(100));
    redraw.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut rate_tick = interval(Duration::from_secs(1));
    rate_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
```

Expand the `tokio::select!` with an arm for `rate_tick.tick()`:

```rust
        tokio::select! {
            // existing arms…
            _ = rate_tick.tick() => {
                state.rate_tick();
                state.expire_flash_if_due();
            }
            _ = redraw.tick() => {}
        }
```

- [ ] **Step 3: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass. Rate tests in state.rs already cover the logic.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/tui/mod.rs crates/sift-cli/src/tui/state.rs
git commit -m "feat(tui): 1 Hz rate-tick + flash expiry in event loop"
```

---

## Task 13: `view_search.rs` rewrite — bordered inputs + throbber

**Goal:** Three stacked bordered input blocks (query/from/to) that change border thickness + color on focus, a throbber during inflight search, `Table` widget for results matching the tail look, level bar reused from middle-bar style.

**Files:**
- Modify: `crates/sift-cli/src/tui/view_search.rs`
- Modify: `crates/sift-cli/src/tui/state.rs` (search_q / search_from / search_to become `Input`)
- Modify: `crates/sift-cli/src/tui/input.rs` (search-mode keys route through tui-input)

- [ ] **Step 1: Migrate search fields to `Input`**

Edit `state.rs`:

```rust
    pub search_q: tui_input::Input,
    pub search_from: tui_input::Input,
    pub search_to: tui_input::Input,
```

Init in `new()`:

```rust
            search_q: tui_input::Input::default(),
            search_from: tui_input::Input::default(),
            search_to: tui_input::Input::default(),
```

Update every reader of these fields to call `.value()`. Grep for `state.search_q\b` etc. and fix.

- [ ] **Step 2: Update `input.rs::search`**

Replace the push/pop helpers with tui-input routing:

```rust
fn search(state: &mut AppState, key: KeyEvent) {
    use super::state::{SearchFocus, SearchStatus};

    match key.code {
        KeyCode::Esc => state.set_mode(Mode::Tail),
        KeyCode::Tab => state.cycle_search_focus(true),
        KeyCode::BackTab => state.cycle_search_focus(false),
        KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => apply_quick_range(state, 3_600),
        KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::ALT) => apply_quick_range(state, 86_400),
        KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::ALT) => apply_quick_range(state, 7 * 86_400),
        KeyCode::Up => state.search_cursor_up(),
        KeyCode::Down => state.search_cursor_down(),
        KeyCode::Enter => match state.search_focus {
            SearchFocus::Query => state.search_status = SearchStatus::Running,
            _ => state.cycle_search_focus(true),
        },
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
        _ => {
            let input = match state.search_focus {
                SearchFocus::Query => &mut state.search_q,
                SearchFocus::From => &mut state.search_from,
                SearchFocus::To => &mut state.search_to,
            };
            crate::tui::record_input::feed(input, key);
        }
    }
}

// Replace any push_focused / pop_focused helpers — they're gone.
```

Update `apply_quick_range` to write through `Input`:

```rust
fn apply_quick_range(state: &mut super::state::AppState, seconds_back: i64) {
    use chrono::Utc;
    let now = Utc::now();
    let lo = now - chrono::Duration::seconds(seconds_back);
    state.search_from = tui_input::Input::new(lo.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    state.search_to = tui_input::Input::new(now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    state.search_status = super::state::SearchStatus::Running;
}
```

- [ ] **Step 3: Rewrite `view_search.rs`**

```rust
//! Search view — three bordered inputs (query/from/to), optional throbber
//! during inflight, Table of results, level bar, and the status line.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Title;
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use sift_core::{Level, Record};
use throbber_widgets_tui::{Throbber, ThrobberState, BRAILLE_SIX};

use super::focus::Pane;
use super::state::{AppState, SearchFocus, SearchStatus};

pub fn draw_search(f: &mut Frame, area: Rect, state: &AppState) {
    let outer = outer_block(state);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let [query_area, fromto_area, status_area, results_area, levels_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(3),
    ])
    .areas(inner);

    draw_input_block(
        f,
        query_area,
        " query ",
        state.search_q.value(),
        state.search_focus == SearchFocus::Query,
        state.focus_pane == Pane::SearchQuery,
        state,
    );

    let [from_area, to_area] =
        Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(fromto_area);

    draw_input_block(
        f,
        from_area,
        " from ",
        state.search_from.value(),
        state.search_focus == SearchFocus::From,
        state.focus_pane == Pane::SearchFrom,
        state,
    );
    draw_input_block(
        f,
        to_area,
        " to ",
        state.search_to.value(),
        state.search_focus == SearchFocus::To,
        state.focus_pane == Pane::SearchTo,
        state,
    );

    draw_status(f, status_area, state);
    draw_results(f, results_area, state);
    super::view_search_levels::draw_levels(f, levels_area, state);
}

fn outer_block(state: &AppState) -> Block<'static> {
    let tabs = Line::from(vec![
        Span::raw(" sift  •  "),
        Span::styled("Tail", Style::default().fg(state.theme.muted)),
        Span::raw(" · "),
        Span::styled(
            "⟨ Search ⟩",
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);

    let meta = Line::from(match &state.search_status {
        SearchStatus::Ok { scanned, elapsed_ms } => vec![
            Span::raw(" "),
            Span::styled(
                format!("{} results", state.filtered_search().len()),
                Style::default().fg(state.theme.muted),
            ),
            Span::styled("  •  ", Style::default().fg(state.theme.muted)),
            Span::styled(
                format!("scanned {scanned}"),
                Style::default().fg(state.theme.muted),
            ),
            Span::styled("  •  ", Style::default().fg(state.theme.muted)),
            Span::styled(
                format!("{elapsed_ms} ms"),
                Style::default().fg(state.theme.muted),
            ),
            Span::raw(" "),
        ],
        _ => vec![Span::raw(" ")],
    });

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(state.theme.border))
        .title(tabs)
        .title(Title::from(meta).alignment(Alignment::Right))
}

fn draw_input_block(
    f: &mut Frame,
    area: Rect,
    title: &'static str,
    value: &str,
    search_focus_match: bool,
    pane_focus: bool,
    state: &AppState,
) {
    let is_focused = search_focus_match || pane_focus;
    let border_type = if is_focused {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };
    let border_style = if is_focused {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(title, Style::default().fg(state.theme.muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let marker = if is_focused { "▸ " } else { "  " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(marker, Style::default().fg(state.theme.accent)),
            Span::raw(value.to_string()),
        ])),
        inner,
    );
}

fn draw_status(f: &mut Frame, area: Rect, state: &AppState) {
    let line = match &state.search_status {
        SearchStatus::Idle => Line::from(""),
        SearchStatus::Running => {
            let mut throbber_state = ThrobberState::default();
            throbber_state.calc_next();
            let throbber = Throbber::default()
                .label("searching…")
                .style(Style::default().fg(state.theme.accent))
                .throbber_set(BRAILLE_SIX);
            f.render_stateful_widget(throbber, area, &mut throbber_state);
            return;
        }
        SearchStatus::Ok { .. } => Line::from(""), // meta already in title
        SearchStatus::Err(e) => Line::from(vec![Span::styled(
            format!("error: {e}"),
            Style::default().fg(state.theme.flash_err),
        )]),
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_results(f: &mut Frame, area: Rect, state: &AppState) {
    let recs = state.filtered_search();
    if recs.is_empty() {
        let msg = match state.search_status {
            SearchStatus::Idle => "enter a query and press Enter",
            SearchStatus::Running => "",
            SearchStatus::Ok { .. } => "no results",
            SearchStatus::Err(_) => "",
        };
        f.render_widget(
            Paragraph::new(msg).style(Style::default().fg(state.theme.muted)),
            area,
        );
        return;
    }

    let visible = (area.height as usize).saturating_sub(1).max(1);
    let cursor = state.search_cursor.min(recs.len().saturating_sub(1));
    let (start, end) =
        crate::tui::view_tail::window_around_cursor(recs.len(), visible, cursor);

    let header = Row::new(vec![
        Cell::from("time"),
        Cell::from("host"),
        Cell::from("app"),
        Cell::from("lvl"),
        Cell::from("body"),
    ])
    .style(Style::default().fg(state.theme.muted));

    let rows: Vec<Row> = recs[start..end]
        .iter()
        .enumerate()
        .map(|(i, r)| row(r, start + i == cursor, state))
        .collect();

    let widths = [
        Constraint::Length(19),
        Constraint::Length(10),
        Constraint::Length(7),
        Constraint::Length(5),
        Constraint::Min(20),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(
            Style::default()
                .bg(if state.focus_pane == Pane::SearchResults {
                    state.theme.bg_selected
                } else {
                    state.theme.bg_selected_unfocused
                })
                .fg(state.theme.fg_selected),
        )
        .highlight_symbol("▕ ")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    f.render_widget(table, area);
}

fn row<'a>(r: &'a Record, _selected: bool, state: &AppState) -> Row<'a> {
    let ts = r.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
    let level_cell = level_cell(r.level, state);
    Row::new(vec![
        Cell::from(ts).style(Style::default().fg(state.theme.muted)),
        Cell::from(r.host.clone()),
        Cell::from(r.app.clone()),
        level_cell,
        Cell::from(r.body.clone()),
    ])
}

fn level_cell(lvl: Level, state: &AppState) -> Cell<'static> {
    let color = state.theme.level_color(lvl);
    let text = match lvl {
        Level::Trace => "░░░",
        Level::Debug => "▒▒▒",
        _ => "███",
    };
    Cell::from(text).style(Style::default().fg(color))
}
```

Create a small sibling `view_search_levels.rs` for the levels bar so we don't duplicate the middle-bar logic:

```rust
//! Level bar for search mode — reuses the tail-mode `draw_levels` shape but
//! as its own block (no filter/rate neighbours).

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

use super::focus::Pane;
use super::state::AppState;

pub fn draw_levels(f: &mut Frame, area: Rect, state: &AppState) {
    let focused = state.focus_pane == Pane::LevelsBar;
    let border_type = if focused { BorderType::Thick } else { BorderType::Rounded };
    let border_style = if focused {
        Style::default().fg(state.theme.border_focused)
    } else {
        Style::default().fg(state.theme.border)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(" levels ", Style::default().fg(state.theme.muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [pills_area] = Layout::horizontal([Constraint::Min(1)]).areas(inner);

    let set = state.level_set;
    let pills = [
        ('I', set.info, state.theme.level_info),
        ('W', set.warn, state.theme.level_warn),
        ('E', set.error, state.theme.level_error),
        ('F', set.fatal, state.theme.level_fatal),
        ('D', set.debug, state.theme.level_debug),
        ('T', set.trace, state.theme.level_trace),
    ];
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(pills.len() * 3);
    for (i, (letter, on, color)) in pills.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        let dot = if *on { "●" } else { "○" };
        spans.push(Span::styled(
            dot.to_string(),
            Style::default().fg(if *on { *color } else { state.theme.disabled }),
        ));
        spans.push(Span::styled(
            letter.to_string(),
            if *on {
                Style::default().fg(*color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(state.theme.disabled)
            },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), pills_area);
}
```

Register in `mod.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-cli/src/tui/
git commit -m "feat(tui): view_search — bordered inputs + throbber + Table results"
```

---

## Task 14: `view_detail.rs` rewrite — syntect JSON + nested sections

**Goal:** Replace the current flat detail popup with nested bordered blocks, and render the `fields` JSON via `syntect`. Falls back to `jsonfmt::highlight` if syntect fails for any reason.

**Files:**
- Modify: `crates/sift-cli/src/tui/view_detail.rs`
- Create: `crates/sift-cli/src/tui/syntax.rs` — syntect adapter module

- [ ] **Step 1: Create `syntax.rs`**

```rust
//! Syntect → Ratatui adapter. Renders a string as a `Vec<Line<'static>>` using
//! a named theme from `syntect::highlighting::ThemeSet::load_defaults`.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SynColor, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Highlight `source` as JSON using the given theme name. Returns None on any
/// error (unknown theme, syntax missing, etc.) so the caller can fall back.
#[must_use]
pub fn highlight_json(source: &str, theme_name: &str) -> Option<Vec<Line<'static>>> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ss.find_syntax_by_extension("json")?;
    let theme = ts.themes.get(theme_name)?;
    let mut h = HighlightLines::new(syntax, theme);

    let mut out: Vec<Line<'static>> = Vec::new();
    for line in source.lines() {
        let ranges = h.highlight_line(line, &ss).ok()?;
        let spans: Vec<Span<'static>> = ranges
            .into_iter()
            .map(|(style, text)| {
                Span::styled(
                    text.to_string(),
                    Style::default().fg(to_ratatui_color(style.foreground)),
                )
            })
            .collect();
        out.push(Line::from(spans));
    }
    Some(out)
}

fn to_ratatui_color(c: SynColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_trivial_json_object() {
        let lines = highlight_json("{\"n\":1}", "base16-ocean.dark");
        assert!(lines.is_some());
        let lines = lines.unwrap();
        assert!(!lines.is_empty());
    }

    #[test]
    fn unknown_theme_returns_none() {
        assert!(highlight_json("{}", "no-such-theme-12345").is_none());
    }

    #[test]
    fn empty_input_returns_empty_vec() {
        let lines = highlight_json("", "base16-ocean.dark").unwrap();
        assert!(lines.is_empty());
    }
}
```

Register in `mod.rs`:

```rust
mod syntax;
```

- [ ] **Step 2: Rewrite `view_detail.rs`**

```rust
//! Record detail popup — 80%×80%, nested bordered sections, syntect JSON.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::block::Title;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;
use sift_core::Record;

use super::state::AppState;

pub fn draw_detail(f: &mut Frame, r: &Record) {
    let theme = super::theme::Theme::dark(); // detail popup doesn't read AppState directly
    draw_detail_with_theme(f, r, &theme);
}

fn draw_detail_with_theme(f: &mut Frame, r: &Record, theme: &super::theme::Theme) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focused))
        .title(detail_title(r, theme))
        .title(Title::from(" × ").alignment(Alignment::Right));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let [header, body_area, labels_area, fields_area, raw_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Min(4),
        Constraint::Length(4),
    ])
    .areas(inner);

    render_header(f, header, r, theme);
    render_body(f, body_area, r, theme);
    render_labels(f, labels_area, r, theme);
    render_fields(f, fields_area, r, theme);
    render_raw(f, raw_area, r, theme);
}

fn detail_title<'a>(r: &Record, theme: &super::theme::Theme) -> Line<'a> {
    Line::from(vec![
        Span::raw(" record · "),
        Span::styled(r.timestamp.to_rfc3339(), Style::default().fg(theme.muted)),
        Span::raw(" · "),
        Span::raw(r.host.clone()),
        Span::raw(" · "),
        Span::raw(r.app.clone()),
        Span::raw(" · "),
        Span::styled(
            format!("{:?}", r.level),
            Style::default()
                .fg(theme.level_color(r.level))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ])
}

fn render_header(_f: &mut Frame, _area: Rect, _r: &Record, _theme: &super::theme::Theme) {
    // Reserved row — title carries the record identity.
}

fn render_body(f: &mut Frame, area: Rect, r: &Record, theme: &super::theme::Theme) {
    let block = section_block(" body ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(r.body.clone()).wrap(Wrap { trim: false }),
        inner,
    );
}

fn render_labels(f: &mut Frame, area: Rect, r: &Record, theme: &super::theme::Theme) {
    let block = section_block(" labels ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if r.labels.is_empty() {
        f.render_widget(
            Paragraph::new("(none)").style(Style::default().fg(theme.muted)),
            inner,
        );
        return;
    }

    let lines: Vec<Line<'_>> = r
        .labels
        .iter()
        .map(|(k, v)| {
            Line::from(vec![
                Span::styled(
                    k.clone(),
                    Style::default()
                        .fg(theme.muted)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(v.clone()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_fields(f: &mut Frame, area: Rect, r: &Record, theme: &super::theme::Theme) {
    let block = section_block(" fields ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if r.fields.is_empty() {
        f.render_widget(
            Paragraph::new("(none)").style(Style::default().fg(theme.muted)),
            inner,
        );
        return;
    }

    let value = serde_json::to_value(&r.fields).unwrap_or(serde_json::Value::Null);
    let pretty = serde_json::to_string_pretty(&value).unwrap_or_default();

    // Try syntect first; fall back to jsonfmt on any failure.
    let lines = super::syntax::highlight_json(&pretty, theme.highlighter_theme)
        .unwrap_or_else(|| super::jsonfmt::highlight(&value));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn render_raw(f: &mut Frame, area: Rect, r: &Record, theme: &super::theme::Theme) {
    let block = section_block(" raw ", theme);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let text = r.raw.clone().unwrap_or_else(|| "(none)".into());
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), inner);
}

fn section_block(title: &'static str, theme: &super::theme::Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Span::styled(title, Style::default().fg(theme.muted)))
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [_, h, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(v);

    h
}
```

Note on the `draw_detail_with_theme` → `draw_detail` split: in practice you may want to pass `state.theme` into `draw_detail`, which means changing `view.rs` to pass `state` into `draw_detail` instead of `r`. Do whichever is simpler compile-wise. The stub `Theme::dark()` shortcut is fine as a first pass; full state integration happens in Task 17 when focus cycling arrives.

- [ ] **Step 3: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass. New syntax tests confirm syntect loads.

- [ ] **Step 4: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/tui/syntax.rs crates/sift-cli/src/tui/view_detail.rs crates/sift-cli/src/tui/mod.rs
git commit -m "feat(tui): view_detail — nested sections + syntect JSON with jsonfmt fallback"
```

---

## Task 15: `view_help.rs` rewrite — KEYBIND_TABLE + two-column grid

**Goal:** Replace the current help popup with a sectioned two-column grid generated from a single `KEYBIND_TABLE` constant. Ensures help can't drift from the actual keybind dispatcher.

**Files:**
- Modify: `crates/sift-cli/src/tui/view_help.rs`

- [ ] **Step 1: Rewrite `view_help.rs`**

```rust
//! Help overlay — two-column keybind grid generated from one constant.

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

use super::state::AppState;

/// Single source of truth for the help overlay. Sections in rendering order.
pub(super) const KEYBIND_TABLE: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("Ctrl+C", "quit"),
            ("?", "toggle help"),
            ("Esc", "close overlay / return to previous focus"),
            (":", "open command prompt"),
            ("Tab / Shift+Tab", "cycle focused pane"),
        ],
    ),
    (
        "Tail · Records",
        &[
            ("j / ↓", "cursor down"),
            ("k / ↑", "cursor up"),
            ("g / Home", "top"),
            ("G / End", "follow tail"),
            ("↵", "open detail"),
            ("Space", "pause / resume"),
            ("Ctrl+T", "toggle absolute / relative timestamps"),
            ("I W E F", "toggle Info / Warn / Error / Fatal"),
            ("D T", "toggle Debug / Trace"),
            ("/", "focus filter"),
        ],
    ),
    (
        "Search",
        &[
            ("t", "back to tail"),
            ("Tab / Shift+Tab", "cycle Query → From → To → Results → Levels"),
            ("↵ (in Query)", "submit"),
            ("Alt+1 / 2 / 3", "quick range 1h / 24h / 7d"),
            ("o", "open detail for selected result"),
        ],
    ),
    (
        "Command mode",
        &[
            (":pause / :resume / :clear / :retry", ""),
            (":save path=<file>", ""),
            (":search q=<str> from=<iso> to=<iso>", ""),
            (":level <name> on|off", ""),
            (":help / :quit", ""),
            ("Tab", "complete"),
            ("Esc", "cancel"),
        ],
    ),
];

pub fn draw_help(f: &mut Frame) {
    let theme = super::theme::Theme::dark();
    let area = centered_rect(70, 90, f.area());
    f.render_widget(Clear, area);

    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border_focused))
        .title(Span::styled(
            " keybindings · Esc or ? to close ",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (title, entries) in KEYBIND_TABLE {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            (*title).to_string(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        for (k, v) in *entries {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(
                    format!("{:<28}", k),
                    Style::default().fg(theme.shortcut_key),
                ),
                Span::raw("  "),
                Span::styled(
                    (*v).to_string(),
                    Style::default().fg(theme.shortcut_label),
                ),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let [_, v, _] = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .areas(area);

    let [_, h, _] = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .areas(v);

    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybind_table_has_every_section() {
        let titles: Vec<&str> = KEYBIND_TABLE.iter().map(|(t, _)| *t).collect();
        assert!(titles.contains(&"Global"));
        assert!(titles.contains(&"Tail · Records"));
        assert!(titles.contains(&"Search"));
        assert!(titles.contains(&"Command mode"));
    }

    #[test]
    fn every_section_has_at_least_one_entry() {
        for (title, entries) in KEYBIND_TABLE {
            assert!(!entries.is_empty(), "section '{title}' is empty");
        }
    }

    #[test]
    fn global_section_documents_ctrl_c_and_question_mark() {
        let (_, global) = KEYBIND_TABLE
            .iter()
            .find(|(t, _)| *t == "Global")
            .expect("Global section exists");
        let keys: Vec<&str> = global.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Ctrl+C"));
        assert!(keys.contains(&"?"));
        assert!(keys.contains(&":"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli tui::view_help`
Expected: 3 new tests pass.

- [ ] **Step 3: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/sift-cli/src/tui/view_help.rs
git commit -m "feat(tui): view_help — KEYBIND_TABLE + two-column grid, sections per mode"
```

---

## Task 16: Timeago integration + Ctrl+T timestamp toggle

**Goal:** Display timestamps as either absolute (`12:34:56.789`) or relative (`2s ago`) depending on `state.timestamp_mode`. Ctrl+T toggles.

**Files:**
- Modify: `crates/sift-cli/src/tui/view_tail.rs`
- Modify: `crates/sift-cli/src/tui/view_search.rs`
- Modify: `crates/sift-cli/src/tui/input.rs`

- [ ] **Step 1: Failing test in `input.rs`**

```rust
    #[test]
    fn ctrl_t_in_tail_list_toggles_timestamp_mode() {
        use super::super::state::TimestampMode;
        let mut s = AppState::new();
        assert_eq!(s.timestamp_mode, TimestampMode::Absolute);
        let ctrl_t = KeyEvent {
            code: KeyCode::Char('t'),
            modifiers: KeyModifiers::CONTROL,
            kind: crossterm::event::KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        };
        dispatch(&mut s, ctrl_t, &dummy_retry());
        assert_eq!(s.timestamp_mode, TimestampMode::Relative);
    }
```

- [ ] **Step 2: Run — verify fail**

Run: `cargo test -p sift-cli tui::input::tests::ctrl_t_in_tail_list_toggles_timestamp_mode`
Expected: FAIL (Ctrl+T currently unhandled).

- [ ] **Step 3: Add dispatch in `input.rs`**

Inside `dispatch`, before the mode/focus match, add:

```rust
    // Ctrl+T toggles abs/rel timestamps (global, any mode).
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('t')) {
        state.toggle_timestamp_mode();
        return;
    }
```

Place this AFTER the command-prompt / help / detail guards but BEFORE the `?` / `:` / `r` checks.

- [ ] **Step 4: Helper to format a timestamp per mode**

Add in `view_tail.rs` (or better, a new tiny `view_tail_ts.rs` / into existing view_tail):

```rust
pub(super) fn format_ts(ts: chrono::DateTime<chrono::Utc>, mode: crate::tui::state::TimestampMode) -> String {
    use crate::tui::state::TimestampMode;
    match mode {
        TimestampMode::Absolute => ts.format("%H:%M:%S%.3f").to_string(),
        TimestampMode::Relative => {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(ts);
            // chrono::Duration → std::time::Duration for timeago::Formatter.
            let std_dur = if diff.num_milliseconds() >= 0 {
                std::time::Duration::from_millis(u64::try_from(diff.num_milliseconds()).unwrap_or(0))
            } else {
                std::time::Duration::ZERO
            };
            timeago::Formatter::new().convert(std_dur)
        }
    }
}
```

Update the `row()` in `view_tail.rs`:

```rust
    let ts = format_ts(r.timestamp, state.timestamp_mode);
```

Update the `row()` in `view_search.rs` (same substitution).

- [ ] **Step 5: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/src/tui/
git commit -m "feat(tui): Ctrl+T toggles absolute/relative timestamps (via timeago)"
```

---

## Task 17: Focus cycling — Tab/Shift+Tab + vim hjkl + LevelsBar pill cursor

**Goal:** Wire the `focus::cycle` logic into `input.rs` and add vim-style `h/j/k/l` aliases in `Records` focus. Move between level pills with `h`/`l` when `LevelsBar` is focused.

**Files:**
- Modify: `crates/sift-cli/src/tui/input.rs`
- Modify: `crates/sift-cli/src/tui/state.rs` (level pill cursor)

- [ ] **Step 1: Add pill cursor state**

Edit `state.rs`. Add a field to `AppState`:

```rust
    pub level_pill_cursor: usize,  // 0..6 across [I W E F D T]
```

Init in `new()`:

```rust
            level_pill_cursor: 0,
```

Add methods:

```rust
    pub fn level_pill_left(&mut self) {
        self.level_pill_cursor = self.level_pill_cursor.saturating_sub(1);
    }

    pub fn level_pill_right(&mut self) {
        if self.level_pill_cursor < 5 {
            self.level_pill_cursor += 1;
        }
    }

    pub fn toggle_current_level_pill(&mut self) {
        use sift_core::Level;
        let lvl = match self.level_pill_cursor {
            0 => Level::Info,
            1 => Level::Warn,
            2 => Level::Error,
            3 => Level::Fatal,
            4 => Level::Debug,
            _ => Level::Trace,
        };
        self.toggle_level(lvl);
    }
```

- [ ] **Step 2: Failing tests in `input.rs`**

```rust
    #[test]
    fn tab_in_tail_list_cycles_focus_to_filter() {
        use super::super::focus::Pane;
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::Tab), &dummy_retry());
        assert_eq!(s.focus_pane, Pane::Filter);
    }

    #[test]
    fn shift_tab_in_tail_list_cycles_focus_to_levelsbar() {
        use super::super::focus::Pane;
        let mut s = AppState::new();
        dispatch(&mut s, k(KeyCode::BackTab), &dummy_retry());
        assert_eq!(s.focus_pane, Pane::LevelsBar);
    }

    #[test]
    fn j_alias_moves_tail_cursor_down() {
        let mut s = AppState::new();
        use chrono::{TimeZone, Utc};
        use sift_core::{Level, Record};
        use std::collections::BTreeMap;
        for i in 0..3 {
            s.push_record(Record {
                timestamp: Utc.with_ymd_and_hms(2026, 4, 22, 12, 0, i).unwrap(),
                host: "h".into(),
                app: "a".into(),
                stream_id: "s".into(),
                level: Level::Info,
                body: format!("msg {i}"),
                labels: BTreeMap::new(),
                fields: BTreeMap::new(),
                raw: None,
            });
        }
        s.tail_cursor = Some(0);
        dispatch(&mut s, k(KeyCode::Char('j')), &dummy_retry());
        assert_eq!(s.tail_cursor_or_last(), 1);
    }

    #[test]
    fn l_in_levelsbar_moves_pill_cursor_right() {
        use super::super::focus::Pane;
        let mut s = AppState::new();
        s.focus_pane = Pane::LevelsBar;
        assert_eq!(s.level_pill_cursor, 0);
        dispatch(&mut s, k(KeyCode::Char('l')), &dummy_retry());
        assert_eq!(s.level_pill_cursor, 1);
    }

    #[test]
    fn space_in_levelsbar_toggles_pill_at_cursor() {
        use super::super::focus::Pane;
        let mut s = AppState::new();
        s.focus_pane = Pane::LevelsBar;
        // Cursor 0 = Info, which is on by default.
        dispatch(&mut s, k(KeyCode::Char(' ')), &dummy_retry());
        assert!(!s.level_set.info);
    }
```

- [ ] **Step 3: Run — verify fail**

Run: `cargo test -p sift-cli tui::input::tests::tab_in_tail_list_cycles_focus_to_filter`
Expected: FAIL.

- [ ] **Step 4: Wire focus cycling + vim aliases in dispatch**

Edit `input.rs`. Update `dispatch` BEFORE the mode/focus match:

```rust
    // Tab / Shift+Tab cycle the pane focus within the current mode.
    if key.code == KeyCode::Tab {
        state.focus_pane =
            super::focus::cycle(state.focus_pane, state.mode, true);
        return;
    }
    if key.code == KeyCode::BackTab {
        state.focus_pane =
            super::focus::cycle(state.focus_pane, state.mode, false);
        return;
    }
```

Inside `tail_list`, add vim aliases + handle `LevelsBar` focus:

```rust
fn tail_list(state: &mut AppState, key: KeyEvent) {
    use super::focus::Pane;

    // LevelsBar has its own micro-keybinds.
    if state.focus_pane == Pane::LevelsBar {
        match key.code {
            KeyCode::Char('h') | KeyCode::Left => state.level_pill_left(),
            KeyCode::Char('l') | KeyCode::Right => state.level_pill_right(),
            KeyCode::Char(' ') | KeyCode::Enter => state.toggle_current_level_pill(),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') => state.quit(),
        KeyCode::Char('/') => {
            state.focus_filter();
            state.focus_pane = Pane::Filter;
        }
        KeyCode::Char('s') => state.set_mode(Mode::Search),
        KeyCode::Char('t') => state.set_mode(Mode::Tail),
        KeyCode::Up | KeyCode::Char('k') => {
            state.scroll_up(1);
            state.tail_cursor_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.scroll_down(1);
            state.tail_cursor_down();
        }
        KeyCode::PageUp => state.scroll_up(10),
        KeyCode::PageDown => state.scroll_down(10),
        KeyCode::Home | KeyCode::Char('g') => state.scroll_up(usize::MAX / 2),
        KeyCode::End | KeyCode::Char('G') => {
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
        KeyCode::Char(' ') => state.toggle_pause(),
        KeyCode::Char('I') => state.toggle_level(sift_core::Level::Info),
        KeyCode::Char('W') => state.toggle_level(sift_core::Level::Warn),
        KeyCode::Char('E') => state.toggle_level(sift_core::Level::Error),
        KeyCode::Char('F') => state.toggle_level(sift_core::Level::Fatal),
        KeyCode::Char('D') => state.toggle_level(sift_core::Level::Debug),
        KeyCode::Char('T') => state.toggle_level(sift_core::Level::Trace),
        _ => {}
    }
}
```

Note: `/` now also sets `focus_pane = Pane::Filter` so the focus system and the old `Focus::Filter` stay in sync. Similarly update `unfocus_filter` in `state.rs` to reset focus_pane:

```rust
    pub fn unfocus_filter(&mut self) {
        self.focus = Focus::List;
        self.focus_pane = crate::tui::focus::Pane::Records;
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass.

- [ ] **Step 6: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/src/tui/
git commit -m "feat(tui): Tab focus cycling + vim hjkl aliases + LevelsBar pill cursor"
```

---

## Task 18: Flash messages + `CommandAction` dispatch

**Goal:** Consume `state.pending_command` in the event loop: each `CommandAction` triggers the corresponding state change and a flash message. Wires all nine verbs end-to-end.

**Files:**
- Modify: `crates/sift-cli/src/tui/mod.rs` (consume pending_command)
- Modify: `crates/sift-cli/src/tui/state.rs` (clear_records helper; save_records_to_path helper)

- [ ] **Step 1: Add helpers to `state.rs`**

```rust
    pub fn clear_records(&mut self) {
        let cleared = self.records.len();
        self.records.clear();
        self.pending.clear();
        self.tail_cursor = None;
        self.set_flash_ok(format!("cleared {cleared} records"));
    }

    /// Serialize current records (tail mode) or search results (search mode)
    /// to NDJSON at the given path. Flash message on completion.
    pub fn save_to_path(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let source: Vec<&sift_core::Record> = match self.mode {
            Mode::Tail => self.records.iter().collect(),
            Mode::Search => self.search_results.iter().collect(),
        };
        let n = source.len();
        let f = std::fs::File::create(path)?;
        let mut w = std::io::BufWriter::new(f);
        for r in source {
            let line = serde_json::to_string(r)?;
            writeln!(w, "{line}")?;
        }
        w.flush()?;
        self.set_flash_ok(format!("saved {n} records to {}", path.display()));
        Ok(())
    }
```

- [ ] **Step 2: Failing test in `state.rs`**

```rust
    #[test]
    fn clear_records_wipes_buffer_and_sets_flash() {
        let mut s = AppState::new();
        s.push_record(rec("a"));
        s.push_record(rec("b"));
        s.clear_records();
        assert_eq!(s.record_count(), 0);
        assert!(matches!(s.flash.as_ref().map(|f| f.kind), Some(FlashKind::Ok)));
    }

    #[test]
    fn save_to_path_writes_ndjson() {
        use std::io::Read;
        let mut s = AppState::new();
        s.push_record(rec("alpha"));
        s.push_record(rec("beta"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("records.ndjson");
        s.save_to_path(&path).unwrap();
        let mut contents = String::new();
        std::fs::File::open(&path).unwrap().read_to_string(&mut contents).unwrap();
        assert_eq!(contents.lines().count(), 2);
    }
```

Requires `tempfile` in `[dev-dependencies]` — already present in sift-cli's tree? Check Cargo.toml. If not, add:

```toml
[dev-dependencies]
tempfile = "3"
```

(`tempfile 3` is the latest stable per crates.io.)

- [ ] **Step 3: Dispatch `pending_command` in `mod.rs`**

In the main event-loop iteration, right before the next `terminal.draw`, process any pending command:

```rust
        // Drain any pending command from the `:` palette and dispatch it.
        if let Some(action) = state.pending_command.take() {
            dispatch_command(&mut state, action, &retry_tx);
        }
```

Add the dispatcher at the bottom of `mod.rs`:

```rust
fn dispatch_command(
    state: &mut state::AppState,
    action: command::CommandAction,
    retry_tx: &tokio::sync::mpsc::Sender<()>,
) {
    use command::CommandAction as A;
    match action {
        A::Pause => {
            state.paused = true;
            state.set_flash_ok("paused");
        }
        A::Resume => {
            state.resume();
            state.set_flash_ok("resumed");
        }
        A::Clear => state.clear_records(),
        A::Retry => {
            let _ = retry_tx.try_send(());
            state.set_flash_ok("retry triggered");
        }
        A::Save { path } => match state.save_to_path(&path) {
            Ok(()) => {} // flash already set by save_to_path
            Err(e) => state.set_flash_err(format!("save failed: {e}")),
        },
        A::Search { q, from, to } => {
            if let Some(q) = q {
                state.search_q = tui_input::Input::new(q);
            }
            if let Some(f) = from {
                state.search_from = tui_input::Input::new(
                    f.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                );
            }
            if let Some(t) = to {
                state.search_to = tui_input::Input::new(
                    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                );
            }
            state.set_mode(state::Mode::Search);
            state.search_status = state::SearchStatus::Running;
            state.set_flash_ok("search submitted");
        }
        A::Level { level, on } => {
            let current = state.level_set.allows(level);
            if current != on {
                state.toggle_level(level);
            }
            state.set_flash_ok(format!(
                "{level:?} {}",
                if on { "on" } else { "off" }
            ));
        }
        A::Help => {
            state.help_open = true;
        }
        A::Quit => state.quit(),
    }
}
```

Make sure `mod.rs` has:

```rust
mod command;
```

…already declared (was added in Task 6). Also import `state` as a local shortcut if it wasn't.

- [ ] **Step 4: Run tests**

Run: `cargo fmt --all && cargo test -p sift-cli`
Expected: all pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy -p sift-cli --all-targets -- -D warnings -W clippy::pedantic`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/sift-cli/src/tui/ crates/sift-cli/Cargo.toml Cargo.lock
git commit -m "feat(tui): wire CommandAction dispatch + :save NDJSON export"
```

---

## Task 19: Update `crates/sift-cli/README.md` with new keybinds + command palette

**Goal:** Docs catch up with implementation. README reflects the new keybind map, the `:` palette verbs, and the Ctrl+T timestamp toggle.

**Files:**
- Modify: `crates/sift-cli/README.md`

- [ ] **Step 1: Update README**

Replace the TUI keybinds section of `crates/sift-cli/README.md`:

```markdown
## TUI keybinds

### Global
| Key | Action |
|---|---|
| `Ctrl+C` | quit |
| `?` | toggle help popup |
| `Esc` | close overlay / return to previous focus |
| `:` | open command prompt |
| `Ctrl+T` | toggle absolute / relative timestamps |
| `Tab` / `Shift+Tab` | cycle focused pane within the current mode |
| `t` | switch to tail mode |
| `s` | switch to search mode |

### Tail mode — `Records` focus
| Key | Action |
|---|---|
| `q` | quit |
| `/` | focus the filter input |
| `Space` | pause / resume |
| `↑` / `↓` / `k` / `j` | move selection |
| `PageUp` / `PageDown` | scroll 10 |
| `Home` / `g` | scroll to top |
| `End` / `G` | follow tail |
| `Enter` | open detail popup |
| `I` / `W` / `E` / `F` | toggle Info / Warn / Error / Fatal |
| `D` / `T` | toggle Debug / Trace |
| `r` | force immediate reconnect attempt |

### Tail mode — `Filter` focus
| Key | Action |
|---|---|
| Any char | append to filter |
| `←` / `→` / `Home` / `End` | cursor movement (via `tui-input`) |
| `Backspace` | delete char before cursor |
| `Esc` / `Enter` | exit filter focus |

### Tail mode — `LevelsBar` focus
| Key | Action |
|---|---|
| `h` / `l` / `←` / `→` | move pill cursor |
| `Space` / `Enter` | toggle current pill |

### Search mode
| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | cycle `Query → From → To → Results → Levels` |
| `Enter` (from Query) | submit |
| `↑` / `↓` | move result cursor |
| `o` | open detail popup for the cursor's record |
| `Alt+1` / `Alt+2` / `Alt+3` | populate quick range 1h / 24h / 7d + submit |
| `I` / `W` / `E` / `F` / `D` / `T` | toggle level visibility |
| `Esc` | return to tail mode |

### Command palette (`:`)

| Verb | Args | Effect |
|---|---|---|
| `:pause` | — | pause live tail |
| `:resume` | — | resume live tail + drain pending |
| `:clear` | — | clear the records buffer |
| `:retry` | — | force immediate reconnect |
| `:save` | `path=<file>` | serialize current buffer / results to NDJSON |
| `:search` | `q=<str>`, `from=<iso>`, `to=<iso>` | switch to search mode + submit |
| `:level` | `<name> on|off` | toggle a level's visibility |
| `:help` | — | open help popup |
| `:quit` | — | quit |

`Tab` in the prompt completes the verb prefix; `Esc` cancels; `Enter` dispatches. Flash messages surface results (success in green, errors in red) in the footer for ~2 s.
```

- [ ] **Step 2: Commit**

```bash
git add crates/sift-cli/README.md
git commit -m "docs(cli): README keybinds + command palette reference"
```

---

## Final verification

After all 19 tasks:

- [ ] Run the full test suite: `cargo test --workspace`
- [ ] Run workspace clippy: `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] Rebuild the CLI: `.\deploy.ps1 -Cli` (from repo root)
- [ ] Manually smoke-test the TUI: `.\target\release\sift.exe tui`
  - Tab between panes — borders thicken + color accent on focus
  - `/` focuses filter, arrow keys move cursor inside it, Ctrl+A jumps home
  - `Space` pauses — title shows paused indicator, pending counter ticks up
  - `s` enters search; rate sparkline disappears, bordered inputs appear
  - `:pause` works; `:search q=error` works; `:save path=test.ndjson` writes a file
  - `?` opens help; `Esc` closes; every section rendered
  - `Enter` on a record opens detail; syntect-highlighted fields; `Esc` closes
  - Flash messages expire after ~2 s
- [ ] Write a session note to `C:\Users\vz\Programming\Vault\Sift\30-Sessions\YYYY-MM\YYYY-MM-DD — TUI Overhaul Shipped.md` summarising what changed and what's next.
- [ ] Copy this plan file from `docs/superpowers/plans/` into `Vault/Sift/60-Tasks/Plans/` (leave the repo copy in place).

