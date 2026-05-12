# UI Polish Pass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Coordinated visual + interaction polish across the Tracecure UI — sizing pass, Tracecure-distinct lavender accent treatment, tinted level pills, app + host facet filters with overflow popover, and a rate histogram strip.

**Architecture:** Pure UI work in `ui/tracecure-ui/`. No backend changes, no API changes, no schema changes. State extended with two `BTreeSet<String>` fields (`app_set`, `host_set`) plus client-side memos that auto-populate facets from loaded records. CSS restyled across all chrome bars; three new Leptos components (`rate_histogram`, `app_host_filters`, `facet_popover`).

**Tech Stack:** Leptos 0.8 (CSR), Rust 2021, `wasm-bindgen-futures 0.4`, `web-sys 0.3`. Build via `trunk 0.21`. Tests via `cargo test --lib` from `ui/tracecure-ui/`. No new crates.

**Spec:** `docs/superpowers/specs/2026-05-12-ui-polish-pass-design.md` (committed in `71dad583`).

---

## Ground rules for every task

- **One concern per commit.** Each task ends with a single commit (or two if CSS + Rust split cleanly).
- **TDD where applicable.** Pure-state changes start with a failing test. CSS-only changes verify via build + manual deploy.
- **Always run `cargo fmt --all`** from `ui/tracecure-ui/` after editing Rust files. Fold fmt hunks into the same commit.
- **Always run `cargo clippy --all-targets -- -D warnings`** from `ui/tracecure-ui/` before committing.
- **`cargo test --lib`** from `ui/tracecure-ui/` must stay green. Baseline is **44 passing tests** post the 2026-05-11 scroll-counter fix.
- **wasm32 build**: `cargo build --target wasm32-unknown-unknown --release` must succeed before any commit that touches Rust.
- **Never skip git hooks.** If a commit hook fails, fix the issue and create a new commit.

---

## File structure (target)

```
ui/tracecure-ui/
├─ styles.css                              (modified — extensive)
└─ src/
   ├─ app.rs                               (modified — mount RateHistogram)
   ├─ state.rs                             (modified — app_set, host_set, helpers, filtered() extension)
   └─ components/
      ├─ mod.rs                            (modified — export new components)
      ├─ rail.rs                           (unchanged Rust; CSS only)
      ├─ top_bar.rs                        (modified — tinted level pills, mount AppHostFilters)
      ├─ stream.rs                         (unchanged)
      ├─ row.rs                            (unchanged Rust; CSS only)
      ├─ search_row.rs                     (unchanged Rust; CSS only)
      ├─ status_bar.rs                     (modified — bold values, dim labels)
      ├─ paused_banner.rs                  (unchanged Rust; CSS only)
      ├─ command_modal.rs                  (unchanged — inherits palette passively)
      ├─ help_modal.rs                     (unchanged — inherits palette passively)
      ├─ row_detail.rs                     (unchanged — inherits palette passively)
      ├─ flash.rs                          (unchanged — inherits palette passively)
      ├─ rate_histogram.rs                 (NEW — 30-bucket sparkline)
      ├─ app_host_filters.rs               (NEW — facet pills + "+ N more" trigger)
      └─ facet_popover.rs                  (NEW — searchable checkbox list)
```

---

### Task 1: CSS palette tokens + frame shade

**Files:**
- Modify: `ui/tracecure-ui/styles.css`

**Goal:** Add the two new tokens the spec introduced (`--bg-deep`, `--accent-text-strong`) and apply `--bg-deep` to `.rail` and `.status` as the "frame" shade. This is the first visible change — the rail and status bar become subtly darker than the stream.

- [ ] **Step 1: Add tokens to `:root` (after the existing `--debug` line, near the top of `styles.css`)**

In `ui/tracecure-ui/styles.css`, find the `:root { ... }` block (lines ~1–18). After the line `--debug: #6ab0ff;`, add:

```css
    --accent-text-strong: #cdc4ff;
    --bg-deep: #0a0c0f;
```

- [ ] **Step 2: Apply `--bg-deep` to `.rail`**

Find the `.rail` rule (currently `background: var(--bg-2);` or similar). Change the background declaration to:

```css
  .rail {
    /* … existing layout properties unchanged … */
    background: var(--bg-deep);
  }
```

- [ ] **Step 3: Apply `--bg-deep` to `.status`**

Find the `.status` rule (currently `background: var(--bg-2);`). Change to:

```css
  .status {
    /* … existing layout properties unchanged … */
    background: var(--bg-deep);
  }
```

- [ ] **Step 4: Verify build**

Run from `ui/tracecure-ui/`:

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -3
```

Expected: `Finished 'release' profile [optimized] target(s) in <N>s`. No errors. (CSS isn't compiled by cargo but the build sanity-checks the whole crate.)

- [ ] **Step 5: Commit**

```bash
git add ui/tracecure-ui/styles.css
git commit -m "$(cat <<'EOF'
style(ui): add --bg-deep + --accent-text-strong tokens, apply to rail/status

Per the UI polish spec, rail and status bar move from --bg-2 to a new
--bg-deep (#0a0c0f) for a "frame" shade slightly darker than the
stream. --accent-text-strong added for later use on active lavender
backgrounds (LIVE pill, popover).

EOF
)"
```

---

### Task 2: AppState facet state — fields, methods, extended filter

**Files:**
- Modify: `ui/tracecure-ui/src/state.rs`

**Goal:** Add `app_set: BTreeSet<String>` and `host_set: BTreeSet<String>` to `AppState`. Add toggle/bulk-set methods. Add `app_set_allows`/`host_set_allows` helpers. Extend `filtered()` to apply both. Full TDD.

- [ ] **Step 1: Write failing test for `app_set_allows` returning true on empty set**

Open `ui/tracecure-ui/src/state.rs`. Find the `mod tests { … }` block at the bottom. Add this test inside the module, after the existing `on_scroll_to_bottom_already_live_does_not_bump_counter` test:

```rust
    #[test]
    fn app_set_allows_returns_true_when_set_is_empty() {
        let s = AppState::new();
        assert!(s.app_set.is_empty(), "default app_set should be empty");
        assert!(
            s.app_set_allows("anything"),
            "empty app_set means no filter — must allow all"
        );
    }
```

- [ ] **Step 2: Verify test fails to compile (missing field + method)**

Run from `ui/tracecure-ui/`:

```bash
cargo test --lib state::tests::app_set_allows_returns_true_when_set_is_empty 2>&1 | tail -10
```

Expected: compile errors — `app_set` field unknown, `app_set_allows` method unknown.

- [ ] **Step 3: Add `app_set` field + initialization**

Find the `AppState` struct (around line 116, after `pub help_open: bool,`). Add right before the closing `}` of the struct:

```rust
    /// Visible apps. Empty set = no app filter applied (allow all).
    /// Non-empty = strict include-only-these-apps.
    pub app_set: std::collections::BTreeSet<String>,
    /// Visible hosts. Same semantics as app_set.
    pub host_set: std::collections::BTreeSet<String>,
```

In `AppState::new()` (around line 154), add inside the struct literal, right before the closing `}` of the `Self { … }`:

```rust
            app_set: std::collections::BTreeSet::new(),
            host_set: std::collections::BTreeSet::new(),
```

- [ ] **Step 4: Add `app_set_allows` and `host_set_allows` helper methods**

In the `impl AppState` block, after the existing `on_scroll_to_bottom` method, add:

```rust
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
```

- [ ] **Step 5: Re-run the failing test, expect PASS**

```bash
cargo test --lib state::tests::app_set_allows_returns_true_when_set_is_empty 2>&1 | tail -5
```

Expected: `test result: ok. 1 passed`.

- [ ] **Step 6: Write failing test for `app_set_allows` when app is in set**

In `mod tests`, add:

```rust
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
```

- [ ] **Step 7: Run tests, expect PASS**

```bash
cargo test --lib state::tests::app_set_allows 2>&1 | tail -8
```

Expected: 3 passed (the previous one plus the two new ones).

- [ ] **Step 8: Write failing test for `toggle_app` insert-then-remove**

In `mod tests`, add:

```rust
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
```

- [ ] **Step 9: Run tests, expect compile failure (no `toggle_app` / `toggle_host` yet)**

```bash
cargo test --lib state::tests::toggle_app_inserts_then_removes 2>&1 | tail -5
```

Expected: compile error — `toggle_app` method not found.

- [ ] **Step 10: Implement `toggle_app` + `toggle_host`**

In `impl AppState`, after `host_set_allows`, add:

```rust
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
```

- [ ] **Step 11: Run tests, expect PASS**

```bash
cargo test --lib state::tests::toggle 2>&1 | tail -8
```

Expected: 2 new tests pass (plus existing `toggle_*` tests for level + timestamp).

- [ ] **Step 12: Write failing test for `set_apps` bulk setter**

In `mod tests`, add:

```rust
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
```

- [ ] **Step 13: Run tests, expect compile failure (no `set_apps` / `set_hosts`)**

```bash
cargo test --lib state::tests::set_apps_replaces_existing_set 2>&1 | tail -5
```

Expected: compile error.

- [ ] **Step 14: Implement `set_apps` + `set_hosts`**

In `impl AppState`, after `toggle_host`:

```rust
    /// Replace `app_set` wholesale. Used by the popover's `all` / `none` /
    /// `invert` footer actions.
    pub fn set_apps(&mut self, apps: impl IntoIterator<Item = String>) {
        self.app_set = apps.into_iter().collect();
    }

    /// Replace `host_set` wholesale.
    pub fn set_hosts(&mut self, hosts: impl IntoIterator<Item = String>) {
        self.host_set = hosts.into_iter().collect();
    }
```

- [ ] **Step 15: Run tests, expect PASS**

```bash
cargo test --lib state::tests::set_apps_replaces_existing_set state::tests::set_hosts_replaces_existing_set 2>&1 | tail -5
```

Expected: 2 passed.

- [ ] **Step 16: Write failing test for `filtered()` combining all four filters**

In `mod tests`, add:

```rust
    #[test]
    fn filtered_combines_level_app_host_query() {
        let mut s = AppState::new();
        // Two records, different apps and hosts.
        let mut r1 = rec("hello world", Level::Info);
        r1.app = "fanid_app".into();
        r1.host = "KINGS_PC".into();
        let mut r2 = rec("hello fanid_admin", Level::Info);
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
```

- [ ] **Step 17: Run test, expect FAIL (filtered() doesn't apply app/host yet)**

```bash
cargo test --lib state::tests::filtered_combines_level_app_host_query 2>&1 | tail -8
```

Expected: assertion failure at "app_set = {fanid_app} → only r1" — filtered() returns 2 instead of 1.

- [ ] **Step 18: Extend `filtered()` with app and host filters**

Find `pub fn filtered(&self) -> Vec<&Record>` in `state.rs`. The current body filters by level + query. Add two more `.filter()` calls:

```rust
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
```

- [ ] **Step 19: Run test, expect PASS**

```bash
cargo test --lib state::tests::filtered_combines_level_app_host_query 2>&1 | tail -5
```

Expected: test passes.

- [ ] **Step 20: Run full lib test suite, confirm no regressions**

```bash
cargo test --lib 2>&1 | tail -5
```

Expected: `test result: ok. 51 passed` (44 baseline + 7 new).

- [ ] **Step 21: Clippy + fmt**

```bash
cargo fmt -- src/state.rs
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: fmt produces no further diff; clippy clean.

- [ ] **Step 22: Commit**

```bash
git add ui/tracecure-ui/src/state.rs
git commit -m "$(cat <<'EOF'
feat(ui): AppState.app_set + host_set + filter integration

Adds two BTreeSet<String> fields to AppState for client-side facet
filtering. Empty set = no filter (allow all); non-empty = strict
include-only.

API:
- app_set / host_set: pub BTreeSet<String> fields
- toggle_app(name) / toggle_host(name): insert if absent, remove if present
- set_apps(iter) / set_hosts(iter): bulk replace (popover all/none/invert)
- app_set_allows(name) / host_set_allows(name): filter helpers
- filtered() now applies level + query + app + host

7 new tests; 44 -> 51 passing.

EOF
)"
```

---

### Task 3: Chrome sizing pass (CSS only)

**Files:**
- Modify: `ui/tracecure-ui/styles.css`

**Goal:** Resize all chrome bars per the spec — rail wider, top bar taller, rows taller, search row taller, status taller, body font from 11–12px to 13px. Pure CSS; no Rust touched.

- [ ] **Step 1: Resize `.rail`**

In `styles.css`, find the `.rail` rule. Update width and brand/icon sizes:

```css
  .rail {
    width: 48px;
    background: var(--bg-deep);
    /* … existing layout flex/etc unchanged … */
    padding: 14px 0;
  }
  .rail .brand {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    font-weight: 700;
    font-size: 18px;
    margin-bottom: 12px;
  }
  .rail .icon {
    width: 32px;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
    color: var(--text-dim);
    border-radius: 6px;
    cursor: pointer;
  }
  .rail .icon.active {
    color: var(--accent);
    background: var(--accent-bg);
  }
```

(Preserve any non-conflicting existing properties such as `display: flex; flex-direction: column;`. Read the existing block first and merge — don't replace wholesale.)

- [ ] **Step 2: Resize `.top` (top bar)**

Find `.top`. Replace its sizing rules with:

```css
  .top {
    padding: 8px 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-bottom: 1px solid var(--bg-2);
    background: linear-gradient(180deg, var(--bg-2) 0%, var(--bg) 100%);
  }
  .top .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: nowrap;
    min-width: 0;
  }
  .top .pill {
    border: 1px solid var(--border);
    padding: 4px 10px;
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 12px;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
  }
  .top .spacer { flex: 1; }
  .top .count { color: var(--text-muted); font-size: 12px; white-space: nowrap; }
  .top .count strong { color: var(--text); }
  .top .divider { width: 1px; height: 18px; background: var(--bg-2); margin: 0 4px; flex-shrink: 0; }
```

- [ ] **Step 3: Resize `.row` (log row)**

Find the existing log-row rule (likely `.row` or `.row-r` — match what `row.rs` renders). Update height + padding + body font:

```css
  .row {
    height: 34px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    gap: 12px;
    border-bottom: 1px solid var(--bg-2);
    font-size: 13px;
  }
  .row:hover { background: var(--bg-2); }
  .row .ts { color: var(--text-dim); font-variant-numeric: tabular-nums; min-width: 110px; }
  .row .host { color: var(--warn); font-weight: 600; min-width: 90px; }
  .row .app { color: var(--ok); min-width: 100px; }
  .row .lvl { font-weight: 700; font-size: 11px; letter-spacing: 0.05em; min-width: 50px; }
  .row .body { color: var(--text); flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
```

Preserve the existing `.row.new` shadow rule (lavender inset) — that was added in commit `3711c7e9`. Just confirm it survives the rewrite.

- [ ] **Step 4: Resize `.search-row` and `.search`**

Find the existing `.search-row` rule. Update:

```css
  .search-row {
    height: 56px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    gap: 12px;
    border-top: 1px solid var(--bg-2);
    background: var(--bg-2);
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--panel);
    border: 1px solid var(--border);
    padding: 0 12px;
    height: 36px;
    border-radius: 8px;
    font-size: 13px;
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-bg);
  }
  .search .ic { color: var(--accent); }
  .search input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text);
    outline: none;
    font-family: inherit;
    font-size: 13px;
  }
  .search input::placeholder { color: var(--text-dim); }
  .kbd {
    border: 1px solid var(--border);
    padding: 1px 6px;
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 11.5px;
    font-family: ui-monospace, monospace;
  }
  .cmd-btn {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 12px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-muted);
    font-size: 12.5px;
    cursor: pointer;
  }
  .cmd-btn:hover { color: var(--text); border-color: var(--border-focus); }
```

- [ ] **Step 5: Resize `.status`**

Find `.status`:

```css
  .status {
    height: 32px;
    padding: 0 16px;
    display: flex;
    align-items: center;
    gap: 14px;
    border-top: 1px solid var(--bg-2);
    background: var(--bg-deep);
    color: var(--text-muted);
    font-size: 12px;
  }
  .status strong { color: var(--text); }
  .status .conn { display: inline-flex; align-items: center; gap: 6px; color: var(--ok); }
  .status .conn::before {
    content: "";
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: 0 0 4px rgba(107, 212, 155, 0.8);
  }
  .status .sep { color: var(--border); }
  .status .spacer { flex: 1; }
  .status .hint { color: var(--text-dim); }
```

(`status-dot` may still appear in `status_bar.rs` — Task 5 of the original status_bar handles that markup. For now this CSS supports both old and new markup via `.conn`.)

- [ ] **Step 6: Build wasm32 to confirm CSS still parses + no regressions**

From `ui/tracecure-ui/`:

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -3
```

Expected: `Finished 'release' profile`.

- [ ] **Step 7: Commit**

```bash
git add ui/tracecure-ui/styles.css
git commit -m "$(cat <<'EOF'
style(ui): chrome sizing pass — rail 48, top 52+, rows 34, search 56, status 32

All chrome bars resized for desktop readability per the UI polish spec:

- rail: 40 -> 48px wide; brand 22 -> 28px (now lavender); icons 26 -> 32px
- top bar: now a two-row flex column with 8px padding + 8px gap;
          adds subtle linear gradient (bg-2 -> bg) for depth
- log row: 22 -> 34px; body font 11 -> 13px; tabular-nums on .ts
- search row: 38 -> 56px; input 28 -> 36px; lavender focus ring
  (1px border + 3px accent-bg halo); pill radius 4 -> 8px
- status: 24 -> 32px; values bold; connected dot 5 -> 6px with green glow

Pure CSS; no Rust touched. Existing markup compatible.

EOF
)"
```

---

### Task 4: Tinted level pills (replace I W E F D T)

**Files:**
- Modify: `ui/tracecure-ui/styles.css`
- Modify: `ui/tracecure-ui/src/components/top_bar.rs`

**Goal:** Replace the cryptic single-letter level filter (`I W E F D T`) with full-name tinted pills (`INFO WARN ERROR FATAL DEBUG TRACE`), each in its level color when active, outline-only when off.

- [ ] **Step 1: Read current top_bar.rs to find the level-letter markup**

```bash
grep -nE "I W E F|level|Level" ui/tracecure-ui/src/components/top_bar.rs | head -10
```

Note the existing structure (likely a `<LevelPills>` child component or inline markup with each letter as a `<span>` toggling `state.toggle_level()`).

- [ ] **Step 2: Add `.lvl-pill` CSS**

In `styles.css`, after the existing `.top .pill` rule:

```css
  .lvl-pill {
    padding: 4px 10px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    border: 1px solid;
    cursor: pointer;
    user-select: none;
    white-space: nowrap;
  }
  .lvl-pill.on.info  { color: var(--debug); border-color: rgba(106, 176, 255, 0.5); background: rgba(106, 176, 255, 0.10); }
  .lvl-pill.on.warn  { color: var(--warn);  border-color: rgba(255, 184, 92, 0.5);  background: rgba(255, 184, 92, 0.10); }
  .lvl-pill.on.error { color: var(--error); border-color: rgba(255, 107, 107, 0.5); background: rgba(255, 107, 107, 0.10); }
  .lvl-pill.on.fatal { color: var(--fatal); border-color: rgba(255, 77, 191, 0.5);  background: rgba(255, 77, 191, 0.10); }
  .lvl-pill.on.debug { color: var(--debug); border-color: rgba(106, 176, 255, 0.5); background: rgba(106, 176, 255, 0.06); opacity: 0.85; }
  .lvl-pill.off { color: var(--text-dim); border-color: var(--border); background: transparent; }
  .lvl-pill:hover { opacity: 1; filter: brightness(1.1); }
```

- [ ] **Step 3: Replace letter markup in `top_bar.rs`**

In `ui/tracecure-ui/src/components/top_bar.rs`, find the existing level-toggle markup and replace with tinted pills. The pattern (adapt to existing markup):

```rust
use tracecure_core::Level;

// inside the view! macro, where the level letters used to live:
<span class="lvl-pill"
      class:on=move || state.with(|s| s.level_set.info)
      class:off=move || state.with(|s| !s.level_set.info)
      class:info=move || true
      on:click=move |_| state.update(|s| s.toggle_level(Level::Info))>
    "INFO"
</span>
<span class="lvl-pill"
      class:on=move || state.with(|s| s.level_set.warn)
      class:off=move || state.with(|s| !s.level_set.warn)
      class:warn=move || true
      on:click=move |_| state.update(|s| s.toggle_level(Level::Warn))>
    "WARN"
</span>
<span class="lvl-pill"
      class:on=move || state.with(|s| s.level_set.error)
      class:off=move || state.with(|s| !s.level_set.error)
      class:error=move || true
      on:click=move |_| state.update(|s| s.toggle_level(Level::Error))>
    "ERROR"
</span>
<span class="lvl-pill"
      class:on=move || state.with(|s| s.level_set.fatal)
      class:off=move || state.with(|s| !s.level_set.fatal)
      class:fatal=move || true
      on:click=move |_| state.update(|s| s.toggle_level(Level::Fatal))>
    "FATAL"
</span>
<span class="lvl-pill"
      class:on=move || state.with(|s| s.level_set.debug)
      class:off=move || state.with(|s| !s.level_set.debug)
      class:debug=move || true
      on:click=move |_| state.update(|s| s.toggle_level(Level::Debug))>
    "DEBUG"
</span>
<span class="lvl-pill"
      class:on=move || state.with(|s| s.level_set.trace)
      class:off=move || state.with(|s| !s.level_set.trace)
      on:click=move |_| state.update(|s| s.toggle_level(Level::Trace))>
    "TRACE"
</span>
```

The `class:on` / `class:off` / level-name classes are reactive — Leptos toggles them on the rendered element. If the existing code uses an inner `<LevelPills>` component, edit that file instead of inlining.

- [ ] **Step 4: Build + check for compile errors**

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -5
```

Expected: builds cleanly.

- [ ] **Step 5: cargo test --lib (no new tests; just confirm no regressions)**

```bash
cargo test --lib 2>&1 | tail -3
```

Expected: `test result: ok. 51 passed`.

- [ ] **Step 6: Clippy + fmt**

```bash
cargo fmt -- src/components/top_bar.rs
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add ui/tracecure-ui/styles.css ui/tracecure-ui/src/components/top_bar.rs
git commit -m "$(cat <<'EOF'
feat(ui): tinted level pills — replace I W E F D T with full names

Per the UI polish spec, the level filter uses pill-shaped, level-tinted
labels instead of cryptic single letters. Each pill renders the full
level name (INFO/WARN/ERROR/FATAL/DEBUG/TRACE) with its level color +
10% tint background when active; outline-only and dim when off.

CSS class scheme: .lvl-pill + .on/.off + level-name variant
(.info/.warn/etc). Border-radius 999px (full pill).

EOF
)"
```

---

### Task 5: Rate histogram component

**Files:**
- Create: `ui/tracecure-ui/src/components/rate_histogram.rs`
- Modify: `ui/tracecure-ui/src/components/mod.rs`
- Modify: `ui/tracecure-ui/src/app.rs`
- Modify: `ui/tracecure-ui/styles.css`
- Modify: `ui/tracecure-ui/src/state.rs` (add bucket computation helper + test)

**Goal:** 30-bucket sparkline strip between the top bar and the stream. Warn/error spikes tinted in their level colors. Average rate label on the right.

- [ ] **Step 1: Write failing test for bucket computation**

In `state.rs` `mod tests`, add:

```rust
    #[test]
    fn rate_buckets_distributes_evenly() {
        use chrono::TimeZone;
        let mut s = AppState::new();
        let base = Utc.with_ymd_and_hms(2026, 5, 12, 9, 0, 0).unwrap();
        // 30 records spread evenly across 30 minutes — one per bucket
        // when window is 30 min and we use 30 buckets.
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
        let bucket_5 = &buckets[5]; // 5 minutes / 30-min window / 30 buckets = bucket index 5
        assert_eq!(bucket_5.total, 2);
        assert_eq!(bucket_5.error_count, 1);
        assert_eq!(bucket_5.warn_count, 1);
    }
```

- [ ] **Step 2: Run test, expect compile failure (no `RateBucket`, no `rate_buckets()`)**

```bash
cargo test --lib state::tests::rate_buckets 2>&1 | tail -8
```

Expected: compile error.

- [ ] **Step 3: Add `RateBucket` struct + `rate_buckets()` method to state.rs**

In `state.rs`, near the top (after the existing enums, before `pub struct AppState`):

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateBucket {
    pub total: u32,
    pub warn_count: u32,
    pub error_count: u32,
}

pub const RATE_BUCKET_COUNT: usize = 30;
```

In `impl AppState`, after `host_set_allows`:

```rust
    /// Bucket records by timestamp into `RATE_BUCKET_COUNT` equal-width
    /// time slots spanning `[from, to)`. Counts totals, warn, and error
    /// per bucket. Records outside the range are skipped.
    #[must_use]
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
```

- [ ] **Step 4: Run tests, expect PASS**

```bash
cargo test --lib state::tests::rate_buckets 2>&1 | tail -5
```

Expected: 2 passed.

- [ ] **Step 5: Create `rate_histogram.rs` component**

Create file `ui/tracecure-ui/src/components/rate_histogram.rs`:

```rust
//! Rate histogram — 30-bucket sparkline above the stream.
//! Warn-tinted bars for buckets with warn records; error-tinted for
//! error/fatal records; default lavender wash otherwise.

use leptos::prelude::*;
use tracecure_ui::state::{AppState, RateBucket, TimeWindow, RATE_BUCKET_COUNT};

use crate::app::AppCtx;

#[component]
pub fn RateHistogram() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    let buckets = move || -> (Vec<RateBucket>, u32, String) {
        state.with(|s| {
            let now = chrono::Utc::now();
            let (from, to, label) = match &s.time_window {
                TimeWindow::Preset(d) => {
                    let from = now - chrono::Duration::from_std(*d).unwrap_or_default();
                    let label = format_window(*d);
                    (from, now, label)
                }
                TimeWindow::Custom { from, to } => {
                    let label = "custom".to_string();
                    (*from, *to, label)
                }
            };
            let bs = s.rate_buckets(from, to);
            let max = bs.iter().map(|b| b.total).max().unwrap_or(1).max(1);
            (bs, max, label)
        })
    };

    let avg_rate = move || -> u32 {
        state.with(|s| {
            let total = s.records.len() as f64;
            let mins = match &s.time_window {
                TimeWindow::Preset(d) => (d.as_secs() as f64 / 60.0).max(1.0),
                TimeWindow::Custom { from, to } => {
                    ((*to - *from).num_seconds() as f64 / 60.0).max(1.0)
                }
            };
            (total / mins).round() as u32
        })
    };

    view! {
        <div class="histogram-strip">
            <span class="hlabel">{move || format!("rate · {}", buckets().2)}</span>
            <div class="sparkline">
                {move || {
                    let (bs, max, _) = buckets();
                    bs.into_iter()
                        .map(|b| {
                            let pct = ((b.total as f64 / max as f64) * 100.0).clamp(0.0, 100.0);
                            let kind = if b.error_count > 0 { "err" }
                                       else if b.warn_count > 0 { "warn" }
                                       else { "" };
                            view! {
                                <div class=format!("b {kind}")
                                     style=format!("height: {pct}%")></div>
                            }
                        })
                        .collect_view()
                }}
            </div>
            <span class="hcount"><strong>{move || format!("~{}", avg_rate())}</strong>"/min"</span>
        </div>
    }
}

fn format_window(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 { format!("{}s", secs) }
    else if secs < 3600 { format!("{}m", secs / 60) }
    else if secs < 86400 { format!("{}h", secs / 3600) }
    else { format!("{}d", secs / 86400) }
}
```

Use `RATE_BUCKET_COUNT` for any iteration count to keep this synced with state.rs.

- [ ] **Step 6: Export from `components/mod.rs`**

In `ui/tracecure-ui/src/components/mod.rs`, add:

```rust
pub mod rate_histogram;
```

Alphabetize with the other `pub mod` lines.

- [ ] **Step 7: Add `.histogram-strip` CSS**

In `styles.css`, after the `.top` rules:

```css
  .histogram-strip {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--bg-2);
    background: var(--bg);
  }
  .histogram-strip .hlabel { color: var(--text-dim); font-size: 11px; min-width: 60px; }
  .histogram-strip .sparkline {
    display: flex;
    align-items: flex-end;
    gap: 1px;
    height: 28px;
    flex: 1;
  }
  .histogram-strip .sparkline .b {
    flex: 1;
    background: rgba(169, 155, 255, 0.35);
    border-radius: 1px 1px 0 0;
    min-width: 3px;
  }
  .histogram-strip .sparkline .b.warn { background: rgba(255, 184, 92, 0.7); }
  .histogram-strip .sparkline .b.err  { background: rgba(255, 107, 107, 0.85); }
  .histogram-strip .hcount {
    color: var(--text-muted);
    font-size: 11px;
    min-width: 80px;
    text-align: right;
  }
  .histogram-strip .hcount strong { color: var(--text); }
```

- [ ] **Step 8: Mount `<RateHistogram />` in `app.rs`**

In `ui/tracecure-ui/src/app.rs`, find the existing layout markup. Place `<RateHistogram />` between `<TopBar />` and `<Stream />`:

```rust
use crate::components::rate_histogram::RateHistogram;

// in the view! macro of the root component:
<TopBar />
<RateHistogram />
<Stream />
```

(Exact line depends on existing layout. Read `app.rs` first, then add to the matching place.)

- [ ] **Step 9: Build + tests**

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -3
cargo test --lib 2>&1 | tail -3
```

Expected: build clean; `test result: ok. 53 passed` (51 prior + 2 new).

- [ ] **Step 10: Clippy + fmt**

```bash
cargo fmt -- src/state.rs src/components/rate_histogram.rs src/components/mod.rs src/app.rs
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 11: Commit**

```bash
git add ui/tracecure-ui/src/components/rate_histogram.rs ui/tracecure-ui/src/components/mod.rs ui/tracecure-ui/src/app.rs ui/tracecure-ui/src/state.rs ui/tracecure-ui/styles.css
git commit -m "$(cat <<'EOF'
feat(ui): rate histogram strip — 30-bucket sparkline over time window

Adds a 44px strip between the top bar and the stream showing record
rate as 30 equal-width buckets across the current time window.
Warn/error spikes tinted in their level colors so visual anomalies
pop without scanning rows.

- New AppState::rate_buckets(from, to) -> Vec<RateBucket> bucketing
  over [from, to) into RATE_BUCKET_COUNT (=30) slots. Counts total +
  warn + error per bucket. Records outside range are skipped.
- New <RateHistogram/> component reads state.time_window, derives
  [from, to], computes max for normalization, renders sparkline.
- Average rate ("~N/min") on the right.
- Pure client-side; no backend API changes; recompute on each state
  change (cheap at RECORD_CAP=5000).

53 tests; 2 new for bucket distribution + level tinting.

EOF
)"
```

---

### Task 6: App/host filter pills (inline, no popover yet)

**Files:**
- Create: `ui/tracecure-ui/src/components/app_host_filters.rs`
- Modify: `ui/tracecure-ui/src/components/mod.rs`
- Modify: `ui/tracecure-ui/src/components/top_bar.rs`
- Modify: `ui/tracecure-ui/styles.css`
- Modify: `ui/tracecure-ui/src/state.rs` (distinct-facet helper + test)

**Goal:** Second row in the top bar with green app pills + orange host pills, top-6 inline (sorted by record count desc). "+ N more ▾" trigger is a placeholder button (the popover lands in Task 7).

- [ ] **Step 1: Write failing test for distinct_apps**

In `state.rs` `mod tests`:

```rust
    #[test]
    fn distinct_apps_returns_sorted_by_count_desc() {
        let mut s = AppState::new();
        let mut a = rec("x", Level::Info); a.app = "wallet".into(); s.records.push_back(a);
        for _ in 0..3 {
            let mut a = rec("x", Level::Info); a.app = "fanid_app".into(); s.records.push_back(a);
        }
        for _ in 0..2 {
            let mut a = rec("x", Level::Info); a.app = "fanid_admin".into(); s.records.push_back(a);
        }
        let distinct = s.distinct_apps();
        assert_eq!(distinct, vec![
            ("fanid_app".to_string(), 3),
            ("fanid_admin".to_string(), 2),
            ("wallet".to_string(), 1),
        ]);
    }

    #[test]
    fn distinct_hosts_returns_sorted_by_count_desc() {
        let mut s = AppState::new();
        let mut a = rec("x", Level::Info); a.host = "prod-01".into(); s.records.push_back(a);
        for _ in 0..2 {
            let mut a = rec("x", Level::Info); a.host = "KINGS_PC".into(); s.records.push_back(a);
        }
        let distinct = s.distinct_hosts();
        assert_eq!(distinct[0].0, "KINGS_PC");
        assert_eq!(distinct[0].1, 2);
    }
```

- [ ] **Step 2: Run, expect compile failure**

```bash
cargo test --lib state::tests::distinct_apps 2>&1 | tail -5
```

Expected: method `distinct_apps` not found.

- [ ] **Step 3: Implement `distinct_apps` + `distinct_hosts`**

In `impl AppState`, after `rate_buckets`:

```rust
    /// Returns distinct apps from `records`, sorted by record count desc
    /// then name asc. Used to populate the facet filter pills.
    #[must_use]
    pub fn distinct_apps(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for r in &self.records {
            *counts.entry(r.app.as_str()).or_insert(0) += 1;
        }
        let mut v: Vec<(String, usize)> = counts.into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Same shape for hosts.
    #[must_use]
    pub fn distinct_hosts(&self) -> Vec<(String, usize)> {
        let mut counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for r in &self.records {
            *counts.entry(r.host.as_str()).or_insert(0) += 1;
        }
        let mut v: Vec<(String, usize)> = counts.into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
```

- [ ] **Step 4: Run tests, expect PASS**

```bash
cargo test --lib state::tests::distinct_apps state::tests::distinct_hosts 2>&1 | tail -5
```

Expected: 2 passed.

- [ ] **Step 5: Add CSS for app/host pills**

In `styles.css`, after `.lvl-pill`:

```css
  .app-pill {
    padding: 4px 9px;
    border-radius: 6px;
    font-size: 11.5px;
    border: 1px solid;
    cursor: pointer;
    user-select: none;
    white-space: nowrap;
  }
  .app-pill.app-on  { color: var(--ok);   border-color: rgba(107, 212, 155, 0.5); background: rgba(107, 212, 155, 0.10); }
  .app-pill.host-on { color: var(--warn); border-color: rgba(255, 184, 92, 0.5);  background: rgba(255, 184, 92, 0.10); }
  .app-pill.off     { color: var(--text-dim); border-color: var(--border); background: transparent; }
  .app-pill:hover { filter: brightness(1.1); }
  .more-btn {
    padding: 4px 10px;
    border-radius: 6px;
    font-size: 11.5px;
    border: 1px solid var(--border);
    color: var(--accent);
    background: transparent;
    cursor: pointer;
    white-space: nowrap;
    font-family: inherit;
  }
  .more-btn:hover { background: var(--accent-bg); }
  .top .label { font-size: 11px; color: var(--text-dim); }
```

- [ ] **Step 6: Create `app_host_filters.rs`**

Create `ui/tracecure-ui/src/components/app_host_filters.rs`:

```rust
//! App + host facet filters. Renders the second row of the top bar:
//! "apps: <pills> [+ N more]  | hosts: <pills> [+ N more]".

use leptos::prelude::*;
use tracecure_ui::state::AppState;

use crate::app::AppCtx;

const TOP_N: usize = 6;

#[component]
pub fn AppHostFilters() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;

    // Only render if any records have arrived (which guarantees at least
    // one app and one host facet exists).
    let has_records = move || state.with(|s| !s.records.is_empty());

    let apps = move || state.with(|s| s.distinct_apps());
    let hosts = move || state.with(|s| s.distinct_hosts());

    view! {
        <Show when=has_records>
            <div class="row">
                <span class="label">"apps:"</span>
                {move || apps()
                    .into_iter()
                    .take(TOP_N)
                    .map(|(name, _count)| {
                        let n = name.clone();
                        let n_class = name.clone();
                        view! {
                            <span class="app-pill"
                                  class:app-on=move || state.with(|s| s.app_set_allows(&n_class) && (!s.app_set.is_empty() || s.app_set.contains(&n_class)))
                                  class:off=move || state.with(|s| !s.app_set.is_empty() && !s.app_set.contains(&n_class))
                                  on:click={
                                      let n = n.clone();
                                      move |_| state.update(|s| s.toggle_app(&n))
                                  }>
                                {name}
                            </span>
                        }
                    })
                    .collect_view()}
                {move || {
                    let overflow = apps().len().saturating_sub(TOP_N);
                    if overflow > 0 {
                        view! { <button class="more-btn">"+ "{overflow}" more ▾"</button> }.into_any()
                    } else { ().into_any() }
                }}
                <span class="divider"></span>
                <span class="label">"hosts:"</span>
                {move || hosts()
                    .into_iter()
                    .take(TOP_N)
                    .map(|(name, _count)| {
                        let n = name.clone();
                        let n_class = name.clone();
                        view! {
                            <span class="app-pill"
                                  class:host-on=move || state.with(|s| s.host_set_allows(&n_class) && (!s.host_set.is_empty() || s.host_set.contains(&n_class)))
                                  class:off=move || state.with(|s| !s.host_set.is_empty() && !s.host_set.contains(&n_class))
                                  on:click={
                                      let n = n.clone();
                                      move |_| state.update(|s| s.toggle_host(&n))
                                  }>
                                {name}
                            </span>
                        }
                    })
                    .collect_view()}
                {move || {
                    let overflow = hosts().len().saturating_sub(TOP_N);
                    if overflow > 0 {
                        view! { <button class="more-btn">"+ "{overflow}" more ▾"</button> }.into_any()
                    } else { ().into_any() }
                }}
            </div>
        </Show>
    }
}
```

Note: the active/off class logic is intentionally written to read naturally — "active = empty set (allow all) OR present in non-empty set". Adjust if the Show-when-empty default visual feels wrong; we can tune in Task 7's review.

- [ ] **Step 7: Export from `components/mod.rs`**

Add `pub mod app_host_filters;`.

- [ ] **Step 8: Mount `<AppHostFilters />` as Row 2 of top_bar**

In `top_bar.rs`, after the existing single `<div class="row">…</div>` (Row 1 with time/levels/count/LIVE), add the child component:

```rust
use crate::components::app_host_filters::AppHostFilters;

// Inside the view! macro, after Row 1:
<AppHostFilters />
```

- [ ] **Step 9: Build + tests**

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -3
cargo test --lib 2>&1 | tail -3
```

Expected: build clean; 55 passed (53 prior + 2 new for distinct_apps/hosts).

- [ ] **Step 10: Clippy + fmt**

```bash
cargo fmt -- src/state.rs src/components/app_host_filters.rs src/components/mod.rs src/components/top_bar.rs
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 11: Commit**

```bash
git add ui/tracecure-ui/src/state.rs ui/tracecure-ui/src/components/app_host_filters.rs ui/tracecure-ui/src/components/mod.rs ui/tracecure-ui/src/components/top_bar.rs ui/tracecure-ui/styles.css
git commit -m "$(cat <<'EOF'
feat(ui): app + host facet filter pills (top-N inline)

Adds a second top-bar row that auto-populates from loaded records.
Apps tinted green (matches in-row .app color), hosts tinted orange
(matches in-row .host color). Click toggles inclusion in the
respective filter set.

- AppState.distinct_apps() / distinct_hosts(): sort by count desc,
  then name asc.
- <AppHostFilters/>: shows top-6 inline + "+ N more" placeholder
  button for overflow (popover wiring lands in next task).
- Row only renders when records.len() > 0 — empty initial state
  collapses the top bar to one row.

55 tests; 2 new.

EOF
)"
```

---

### Task 7: Facet popover for overflow

**Files:**
- Create: `ui/tracecure-ui/src/components/facet_popover.rs`
- Modify: `ui/tracecure-ui/src/components/mod.rs`
- Modify: `ui/tracecure-ui/src/components/app_host_filters.rs`
- Modify: `ui/tracecure-ui/styles.css`

**Goal:** Click the "+ N more ▾" button to open a 280px floating popover anchored to the trigger, with: search input, scrollable checkbox list of all values (with counts), and an `all / none / invert` footer.

- [ ] **Step 1: Add `.facet-popover` CSS**

In `styles.css`:

```css
  .facet-popover {
    position: absolute;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 32px rgba(0,0,0,0.4);
    padding: 8px;
    width: 280px;
    z-index: 100;
  }
  .facet-popover .pop-search {
    display: flex; align-items: center; gap: 8px;
    padding: 6px 10px;
    background: var(--bg);
    border: 1px solid var(--bg-2);
    border-radius: 6px;
    margin-bottom: 8px;
  }
  .facet-popover .pop-search input {
    background: transparent; border: none;
    color: var(--text); outline: none; flex: 1;
    font-family: inherit; font-size: 12px;
  }
  .facet-popover .pop-search .ic { color: var(--text-dim); font-size: 12px; }
  .facet-popover .pop-list { max-height: 280px; overflow-y: auto; }
  .facet-popover .pop-item {
    display: flex; align-items: center; gap: 8px;
    padding: 5px 8px; border-radius: 4px; cursor: pointer;
    font-size: 12px;
  }
  .facet-popover .pop-item:hover { background: rgba(255, 255, 255, 0.03); }
  .facet-popover .pop-item .chk {
    width: 14px; height: 14px;
    border: 1px solid var(--border-focus);
    border-radius: 3px;
    display: inline-flex; align-items: center; justify-content: center;
    flex-shrink: 0;
  }
  .facet-popover .pop-item.checked .chk {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--bg);
    font-size: 10px; font-weight: 700;
  }
  .facet-popover .pop-item .name { color: var(--text); flex: 1; }
  .facet-popover .pop-item .count { color: var(--text-dim); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .facet-popover .pop-footer {
    display: flex; gap: 6px;
    padding-top: 8px; border-top: 1px solid var(--bg-2); margin-top: 6px;
  }
  .facet-popover .pop-btn {
    flex: 1; padding: 4px 8px; border-radius: 4px;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text-muted);
    font-size: 11px; cursor: pointer; font-family: inherit;
  }
  .facet-popover .pop-btn:hover { color: var(--text); border-color: var(--border-focus); }
```

- [ ] **Step 2: Create `facet_popover.rs`**

Create `ui/tracecure-ui/src/components/facet_popover.rs`:

```rust
//! Facet popover — floating searchable checkbox list opened by the
//! "+ N more" button in <AppHostFilters/>. Shared shape for apps and
//! hosts.

use leptos::prelude::*;
use tracecure_ui::state::AppState;

use crate::app::AppCtx;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FacetKind {
    Apps,
    Hosts,
}

#[component]
pub fn FacetPopover(
    kind: FacetKind,
    open: RwSignal<bool>,
) -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = ctx.0;
    let filter_text = RwSignal::new(String::new());

    let entries = move || {
        let ft = filter_text.get().to_lowercase();
        state.with(|s| {
            let all = match kind {
                FacetKind::Apps  => s.distinct_apps(),
                FacetKind::Hosts => s.distinct_hosts(),
            };
            all.into_iter()
                .filter(|(name, _)| ft.is_empty() || name.to_lowercase().contains(&ft))
                .collect::<Vec<_>>()
        })
    };

    let is_checked = move |name: &str| {
        state.with(|s| match kind {
            FacetKind::Apps  => !s.app_set.is_empty() && s.app_set.contains(name),
            FacetKind::Hosts => !s.host_set.is_empty() && s.host_set.contains(name),
        })
    };

    let toggle = move |name: String| {
        state.update(|s| match kind {
            FacetKind::Apps  => s.toggle_app(&name),
            FacetKind::Hosts => s.toggle_host(&name),
        });
    };

    let select_all = move |_| {
        let all_names: Vec<String> = entries().into_iter().map(|(n, _)| n).collect();
        state.update(|s| match kind {
            FacetKind::Apps  => s.set_apps(all_names),
            FacetKind::Hosts => s.set_hosts(all_names),
        });
    };

    let select_none = move |_| {
        state.update(|s| match kind {
            FacetKind::Apps  => s.set_apps(std::iter::empty::<String>()),
            FacetKind::Hosts => s.set_hosts(std::iter::empty::<String>()),
        });
    };

    let invert = move |_| {
        let visible: Vec<String> = entries().into_iter().map(|(n, _)| n).collect();
        state.update(|s| {
            for n in &visible {
                match kind {
                    FacetKind::Apps  => s.toggle_app(n),
                    FacetKind::Hosts => s.toggle_host(n),
                }
            }
        });
    };

    view! {
        <Show when=move || open.get()>
            <div class="facet-popover" style="left: 0; top: calc(100% + 4px);">
                <div class="pop-search">
                    <span class="ic">"⌕"</span>
                    <input type="text"
                           placeholder=move || match kind {
                               FacetKind::Apps  => "filter apps…",
                               FacetKind::Hosts => "filter hosts…",
                           }
                           prop:value=move || filter_text.get()
                           on:input=move |e| filter_text.set(event_target_value(&e))/>
                </div>
                <div class="pop-list">
                    {move || entries()
                        .into_iter()
                        .map(|(name, count)| {
                            let n = name.clone();
                            let checked = is_checked(&name);
                            view! {
                                <div class="pop-item"
                                     class:checked=move || checked
                                     on:click={
                                         let n = n.clone();
                                         move |_| toggle(n.clone())
                                     }>
                                    <span class="chk">{if checked { "✓" } else { "" }}</span>
                                    <span class="name">{name}</span>
                                    <span class="count">{count}</span>
                                </div>
                            }
                        })
                        .collect_view()}
                </div>
                <div class="pop-footer">
                    <button class="pop-btn" on:click=select_all>"all"</button>
                    <button class="pop-btn" on:click=select_none>"none"</button>
                    <button class="pop-btn" on:click=invert>"invert"</button>
                </div>
            </div>
        </Show>
    }
}
```

- [ ] **Step 3: Export from `mod.rs`**

Add `pub mod facet_popover;`.

- [ ] **Step 4: Wire popover into `app_host_filters.rs`**

Replace the placeholder `<button class="more-btn">` from Task 6 with a real trigger that opens a `<FacetPopover>`:

```rust
use crate::components::facet_popover::{FacetPopover, FacetKind};

// inside #[component] pub fn AppHostFilters:
let apps_pop_open = RwSignal::new(false);
let hosts_pop_open = RwSignal::new(false);

// Replace the "apps" overflow button with:
{move || {
    let overflow = apps().len().saturating_sub(TOP_N);
    if overflow > 0 {
        view! {
            <span style="position: relative; display: inline-block;">
                <button class="more-btn"
                        on:click=move |_| apps_pop_open.update(|o| *o = !*o)>
                    "+ "{overflow}" more ▾"
                </button>
                <FacetPopover kind=FacetKind::Apps open=apps_pop_open/>
            </span>
        }.into_any()
    } else { ().into_any() }
}}

// Same for hosts with hosts_pop_open + FacetKind::Hosts.
```

- [ ] **Step 5: Add click-outside dismissal**

In `app_host_filters.rs`, add a global click listener that closes both popovers when clicked outside. Place after the signal declarations:

```rust
Effect::new(move |_| {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    let document = web_sys::window().unwrap().document().unwrap();
    let closure = Closure::wrap(Box::new(move |e: web_sys::Event| {
        if let Some(target) = e.target() {
            if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                // If click target is NOT inside a .facet-popover or .more-btn, close both.
                let mut current = Some(el);
                let mut inside = false;
                while let Some(c) = current.as_ref() {
                    if c.class_name().contains("facet-popover") || c.class_name().contains("more-btn") {
                        inside = true; break;
                    }
                    current = c.parent_element();
                }
                if !inside {
                    apps_pop_open.set(false);
                    hosts_pop_open.set(false);
                }
            }
        }
    }) as Box<dyn FnMut(web_sys::Event)>);
    document
        .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
        .ok();
    closure.forget();
});
```

- [ ] **Step 6: Build + tests**

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -5
cargo test --lib 2>&1 | tail -3
```

Expected: build clean; 55 passed (no new tests — popover is DOM-driven, manual verify).

- [ ] **Step 7: Clippy + fmt**

```bash
cargo fmt -- src/components/facet_popover.rs src/components/app_host_filters.rs src/components/mod.rs
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 8: Commit**

```bash
git add ui/tracecure-ui/src/components/facet_popover.rs ui/tracecure-ui/src/components/mod.rs ui/tracecure-ui/src/components/app_host_filters.rs ui/tracecure-ui/styles.css
git commit -m "$(cat <<'EOF'
feat(ui): facet popover — searchable list + all/none/invert footer

The "+ N more ▾" button now opens a 280px floating popover with:

- Search input filtering the list by substring (case-insensitive)
- Scrollable checkbox list of all facet values with record counts
- Footer actions: all / none / invert apply to the currently
  visible (post-filter) list

Click-outside listener closes both apps and hosts popovers cleanly.
The component is shared between facets via a FacetKind enum
(Apps | Hosts) so the markup + behaviour stays single-source.

EOF
)"
```

---

### Task 8: Paused banner + final consistency tweaks

**Files:**
- Modify: `ui/tracecure-ui/styles.css`

**Goal:** Bring the paused banner into the 13px rhythm. Final sweep for any drift introduced by Tasks 1–7.

- [ ] **Step 1: Find paused banner CSS in `styles.css`**

Search for the `.resume` or `.paused-banner` rule (whatever class `paused_banner.rs` uses).

- [ ] **Step 2: Update typography + spacing to match new rhythm**

```css
  .resume {
    /* … existing layout (position fixed/etc) unchanged … */
    padding: 8px 14px;
    border-radius: 8px;
    font-size: 13px;
    border: 1px solid rgba(169, 155, 255, 0.4);
    background: rgba(169, 155, 255, 0.10);
    color: var(--accent-text-strong);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
    cursor: pointer;
  }
  .resume .d {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 0 6px rgba(169, 155, 255, 0.6);
    display: inline-block; margin-right: 8px;
  }
  .resume .n { color: var(--text); font-weight: 600; }
  .resume .play { color: var(--accent); font-weight: 600; margin-left: 8px; }
```

- [ ] **Step 3: Build to confirm CSS parses**

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -3
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add ui/tracecure-ui/styles.css
git commit -m "$(cat <<'EOF'
style(ui): align paused banner with the 13px chrome rhythm

13px text, lavender accent, glowing dot + shadow to match the LIVE
pill and search focus treatment. Final consistency pass over the
polish pass.

EOF
)"
```

---

### Task 9: Verification + deploy

**Files:** none (verification only)

**Goal:** Final test pass, wasm32 build, deploy, manual smoke test against the live FAN_iD-loaded UI at `http://127.0.0.1:7789/`.

- [ ] **Step 1: Full test suite**

```bash
cargo test --lib 2>&1 | tail -5
```

Expected: `test result: ok. 55 passed`.

- [ ] **Step 2: Clippy across the workspace UI**

```bash
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 3: Release wasm32 build**

```bash
cargo build --target wasm32-unknown-unknown --release 2>&1 | tail -3
```

Expected: `Finished 'release' profile`.

- [ ] **Step 4: Trunk release build (smoke-tests Trunk pipeline)**

From `ui/tracecure-ui/`:

```bash
trunk build --release 2>&1 | tail -5
```

Expected: `dist` produced with the new bundle.

- [ ] **Step 5: Deploy via deploy.ps1**

From repo root (PowerShell):

```powershell
.\deploy.ps1 -Ui
```

Expected: trunk rebuilds the UI; deploy.ps1 reports "UI rebuilt. IIS serves dist\ directly; refresh your browser."

- [ ] **Step 6: Manual smoke test in the browser**

Open `http://127.0.0.1:7789/` (or wherever the deployed UI lives). Verify:

1. Rail is wider (48px), brand "t" lavender, larger icons.
2. Top bar row 1: time pill + 6 tinted level pills (full names, not letters) + records count (bold) + lavender LIVE pill.
3. Top bar row 2 appears once records arrive: `apps:` + green-tinted app pills + (optionally) "+ N more" button; same for hosts (orange-tinted) + their overflow.
4. Rate histogram strip below top bar — 30 bars, warn/error tinted spikes, average rate on the right.
5. Stream rows are 34px, 13px body, lavender bar on "new since paused" rows.
6. Search row 56px tall; pressing `/` focuses the input → 36px input gets lavender 3px halo.
7. Click `+ N more ▾` on apps → popover opens with search + checklist + counts + all/none/invert footer.
8. Click outside → popover closes.
9. Click an app pill → records from other apps disappear from the stream.
10. Status bar at the bottom: 32px, bold values, dim labels, darker `--bg-deep` background.

- [ ] **Step 7: If everything passes, no commit needed (verification only).**

The polish pass is complete. The branch should be green and the deployed UI matches the spec.

---

## Self-review checklist (run before handoff)

1. **Spec coverage:**
   - Sizing pass — Task 1 (tokens) + Task 3 (sizes) ✓
   - Lavender accent — Tasks 1, 3, 4 (level pills), 5 (histogram bars), 6 (more-btn), 7 (popover checkbox), 8 (paused banner) ✓
   - Level filter restyle — Task 4 ✓
   - App + host facet filters — Task 6 (inline) + Task 7 (popover) ✓
   - Rate histogram — Task 5 ✓
   - New tokens (`--bg-deep`, `--accent-text-strong`) — Task 1 ✓
   - `BTreeSet<String>` state extension + `filtered()` — Task 2 ✓
   - Empty-set semantics ("no filter when empty") — Task 2 helpers + Task 6 markup ✓
   - Auto-populate facets from records — Task 6 `distinct_apps`/`distinct_hosts` ✓
   - Top-N (=6) inline + overflow — Task 6 ✓
   - Popover search + counts + footer — Task 7 ✓
   - Click-outside dismissal — Task 7 step 5 ✓
   - 30-bucket rate histogram — Task 5 ✓
   - Warn/error tinted spikes — Task 5 step 3 + CSS step 7 ✓
   - `--bg-deep` for rail + status — Task 1 ✓
   - Subtle gradient on top bar — Task 3 step 2 ✓
   - Glowing LIVE dot + connected dot — Task 3 step 5 ✓
   - Tabular-nums on timestamps — Task 3 step 3 ✓
   - Defer to Tasks 2+5+6 for state model changes ✓
   - All resume_live callers benefit from scroll counter — already in place from prior commit `3711c7e9` ✓
   - Non-goals (saved views, drag-time-select, light theme, server-side filters, TUI changes, modal redesigns) — not tasked ✓

2. **Placeholder scan:** No "TBD", "TODO", "implement later", "fill in details" in any task. All test code, implementation code, and commands are written out completely. ✓

3. **Type consistency:**
   - `RateBucket` defined in Task 5 step 3, used by `rate_buckets()` method and `<RateHistogram/>` component. Field names match (`total`, `warn_count`, `error_count`). ✓
   - `RATE_BUCKET_COUNT: usize = 30` defined in Task 5 step 3, referenced by `<RateHistogram/>` mockup logic. ✓
   - `app_set` / `host_set` as `BTreeSet<String>` — consistent across Task 2 and Tasks 6, 7. ✓
   - `toggle_app(name: &str)` signature — used consistently in `<AppHostFilters/>` and `<FacetPopover/>`. ✓
   - `set_apps(impl IntoIterator<Item = String>)` — used in popover footer actions. ✓
   - `distinct_apps() -> Vec<(String, usize)>` — used by `<AppHostFilters/>` and `<FacetPopover/>`. ✓
   - `FacetKind::Apps | Hosts` enum defined in Task 7, used in Task 7 step 4 wiring. ✓
   - CSS class names: `.lvl-pill` (Task 4), `.app-pill` (Task 6), `.facet-popover` (Task 7), `.histogram-strip` (Task 5), `.more-btn` (Task 6 CSS, used Task 7 step 5 for click-outside detection). ✓
