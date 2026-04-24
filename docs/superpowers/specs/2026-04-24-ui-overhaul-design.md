# UI Overhaul — Design Spec

**Date:** 2026-04-24
**Status:** draft
**Scope:** `ui/sift-ui/` (Leptos 0.6 CSR web app)
**References:** Mezmo (design principles, not chrome). Brainstorm iterations live at `.superpowers/brainstorm/948-1777043807/content/sift-v6.html` (final).

---

## 1. Goal

Replace the current two-route (`/tail`, `/search`) UI with a **single unified log stream** that behaves like a professional observability tool. The current UI has three concrete problems driving this rewrite:

1. **Logs stack unbounded.** Live tail appends records without auto-scroll-to-bottom, without a "jump to latest" affordance, and without windowed rendering — the page grows until the browser chokes.
2. **Tail and search are artificially split.** Browsing "the recent past" forces a mode switch to `/search` instead of simply scrolling up in the stream.
3. **Chrome feels bare.** Visual polish doesn't match the tool's other surfaces (TUI, server). "Not useful, UX is not good."

The new UI is **polish layered on top of fixing these functional issues**, not polish for its own sake. It takes design **principles** from Mezmo (dense monospace log rows, LIVE indicator, dark quiet chrome, logs-as-hero) but does NOT clone Mezmo's SaaS-specific chrome (template library, multi-source taxonomy dropdowns, "Tags/Apps/Sources" splits) because Sift is a self-hosted single-source tool.

## 2. Non-goals (deferred)

- **Saved views panel.** The concept exists conceptually but the backend has no "view" resource. Adding UI for a feature that doesn't exist would be fake chrome. Revisit when a `/v1/views` API is specified.
- **Clickable/draggable timeline histogram.** Without interactivity the timeline is dead chrome. Defer until we wire click-to-jump and drag-to-zoom against the existing query API.
- **Multi-source / multi-tenant selectors.** Sift is single-source today.
- **Alerts UI (M5), plugin UI (M4).** Placeholders in the left rail only.
- **Light theme.** Dark-only for v1.
- **URL-encoded shareable state.** Future; keep URL minimal in v1 (`/` only).
- **Record download / NDJSON export from the UI.** The TUI already has `:save`; UI gets it later.

## 3. User-facing behavior

### 3.1 Unified stream

One page, one route (`/`). No separate tail vs search. The stream displays records inside an **explicit time window** (e.g. "Last 1 hour"). When **LIVE** is on, the window's `to` end rolls forward to `now`; new records stream in at the bottom via WebSocket. When **paused**, the window is frozen.

The `/tail` and `/search` routes are removed. `/search` queries become either (a) live-mode with a tighter window + query filter, or (b) paused-mode with an explicit `from`/`to`.

### 3.2 Scroll + live behavior (Hybrid "C" from brainstorm)

- **Default state on page load:** LIVE on, window = Last 1h, scrolled to bottom (newest records visible).
- **Scroll up (any amount) → auto-pause.** Live stops. A floating pill appears above the bottom search bar: `● Paused · N new records · ▶ Resume live`. `N` ticks upward as new records arrive on the WebSocket while paused.
- **Scroll back to bottom → does NOT auto-resume.** The user keeps their scroll position; the banner stays visible. Prevents "scrolled slightly → flung to bottom" surprises.
- **Explicit resume:** click the banner, click the `● LIVE` button in the top bar, or press `G` / `End`. Any of these (a) clears buffered records into the stream, (b) scrolls to bottom, (c) re-enables live streaming.
- **Time window rolling:** while LIVE, every new record pushes the `to` boundary to `now`. The `from` boundary stays fixed at `now - window_duration` (a sliding window). While paused, both bounds are frozen at the moment of pause.

### 3.3 Time window controls

Top bar has one pill: `◷ Last 1 h ▾`. Clicking opens a dropdown with:

- Presets: `Last 15 min · Last 1 h · Last 6 h · Last 24 h · Last 7 d`
- Custom: "from" datetime-local input + "to" datetime-local input + Apply button. Custom ranges drop out of live (the `to` is explicit, not `now`).

Selecting a preset while paused stays paused (the window changes but live doesn't restart). Selecting a preset while live keeps live on (the window resizes around `now`).

### 3.4 Level pills

Six pills in the top bar: `I W E F D T` (Info, Warn, Error, Fatal, Debug, Trace). Each is either **on** (colored) or **off** (dimmed). Default: I/W/E/F on, D/T off — matches the TUI.

Click a pill to toggle. Keyboard: `I`/`W`/`E`/`F`/`D`/`T` shortcut keys when stream has focus. Toggles apply client-side as filter (no server round-trip; the server sends all levels, the UI hides toggled-off levels).

### 3.5 Query / filter bar (bottom)

Bottom of the screen (not top). Single input field with placeholder: `search / filter — e.g. level:error app:api-prod duration>5s`. Keyboard: `/` focuses.

**v1 syntax:** free-text substring match in the `body` field. (The structured `level:error app:api-prod` hints in the placeholder are aspirational; v1 just does substring. The structured parser is a future enhancement.)

**Filter semantics:**
- **Live mode:** typing filters the already-loaded `records` buffer client-side (no backend round-trip, instant feedback).
- **Windowed mode (custom time range or preset that's not rolling):** typing debounces for ~200ms then fires `GET /v1/search?q=<text>&from=<iso>&to=<iso>&limit=500` to refetch from the backend with the query applied server-side.
- The difference matters because a 5000-record in-memory buffer covers ~the last few minutes at realistic rates; deeper searches need backend help.

Beside the query input: a `[: commands]` button (click to open the command palette). The palette matches the TUI's `:` palette verbs (`:pause`, `:resume`, `:clear`, `:retry`, `:save`, `:search`, `:level`, `:help`, `:quit`). Keyboard: `:` opens.

### 3.6 Record rows

Dense monospace. Each row is one line unless selected (inline expand — see 3.7).

Grid columns (in order):
1. `date` — `HH:MM:SS.mmm` (absolute) or `Ns ago` (relative — toggled by `Ctrl+T` same as TUI). Muted color. Width: ~108px.
2. `host` — accent purple. Truncated with ellipsis. Width: ~100px.
3. `app` — muted text. Width: ~52px.
4. `lvl` — colored per level (green/yellow/red/pink/blue/gray for info/warn/error/fatal/debug/trace). Bold. Width: ~50px.
5. `body` — primary content. Truncated with ellipsis. Flex: 1.

Hover: subtle accent tint. Selected row: accent background. Rows per level get tinted body color (e.g. `.row.error` gets `#ffcccc` body text).

Number literals (`200`, `42ms`) in body are yellow. String literals (`"ops@example.com"`) are green. Identifiers (`ch_1P…zQ`, `a9f7`) are purple. These are static CSS spans generated from a simple tokenizer (re-uses the TUI's `jsonfmt` logic, ported to Rust-for-wasm).

### 3.7 Detail view (inline expand)

Click a row → the row expands inline below itself to show full record detail. Click again or press `Esc` → collapses. Only one row can be expanded at a time.

The expand panel shows (in order):
- Full **body** (word-wrapped).
- **Labels** — table of key/value pairs from `r.labels`.
- **Fields** — `jsonfmt`-highlighted JSON from `r.fields` (ported from the TUI's `jsonfmt` module; lightweight, no syntect-for-wasm bundle bloat — v1 only. Future v2 may swap in syntect-wasm if users want full fidelity).
- **Raw** — `r.raw` if present, monospace, muted.

The expanded panel has a close `×` button in its top-right corner. Pressing Esc collapses it. Pressing `j`/`k` or arrows moves to next/previous row (collapsing and re-expanding).

Inline expand is preferred over a right-side slide-in because:
- Preserves scroll context (the row stays in place).
- No animation lag.
- No extra chrome (no second panel).

### 3.8 Keyboard shortcuts

| Key | Action |
|---|---|
| `/` | Focus query input |
| `:` | Open command palette |
| `?` | Toggle help overlay |
| `Esc` | Close overlay / collapse detail / exit focus |
| `j` / `↓` | Next row |
| `k` / `↑` | Previous row |
| `g` / `Home` | Top of window |
| `G` / `End` | Jump to latest + resume live |
| `Space` | Pause / resume |
| `Enter` | Toggle detail for selected row |
| `I` / `W` / `E` / `F` / `D` / `T` | Toggle level visibility |
| `Ctrl+T` | Toggle absolute / relative timestamps |
| `r` | Force reconnect |
| `Ctrl+C` | Quit (blur the page; actual "quit" is closing the tab) |

Same key table as the TUI's `KEYBIND_TABLE`.

### 3.9 Help overlay (`?`)

Modal centered on viewport. Renders the same KEYBIND_TABLE content as the TUI's help, organized by section (Global / Stream / Command palette). Esc or `?` closes.

### 3.10 Command palette (`:`)

Modal centered near the bottom of the viewport (above the search bar). Single input with `:` prefix pre-filled. Typing matches against the 9 verbs. `Tab` completes the prefix. `Enter` submits. `Esc` cancels.

Dispatches the same `CommandAction` shape the TUI uses. Verbs in v1:
- `:pause` / `:resume` / `:clear` / `:retry` — state changes (local).
- `:save path=<file>` — downloads records as NDJSON via `<a download>`.
- `:search q=<str> from=<iso> to=<iso>` — updates window + query.
- `:level <name> on|off` — toggles a level pill.
- `:help` — opens help.
- `:quit` — closes the tab (or blurs the page, depending on browser).

Bad parses set a flash error message above the bottom bar for ~2s (same as TUI).

## 4. Layout (structural)

Single-page flex column inside a flex row:

```
┌─────┬────────────────────────────────────────────────────────────────────┐
│     │ TOP BAR (44px)                                                      │
│     ├────────────────────────────────────────────────────────────────────┤
│     │                                                                    │
│ R   │                                                                    │
│ A   │ STREAM (flex: 1; overflow-y: auto)                                 │
│ I   │   (paused banner floats above the search row when paused)          │
│ L   │                                                                    │
│     │                                                                    │
│ 40  │                                                                    │
│ px  ├────────────────────────────────────────────────────────────────────┤
│     │ SEARCH ROW (38px): [query input / commands]                        │
│     ├────────────────────────────────────────────────────────────────────┤
│     │ STATUS BAR (24px): connected · records · window · keyboard hint    │
└─────┴────────────────────────────────────────────────────────────────────┘
```

**Top bar (44px):** `◷ Last 1h ▾` | `I W E F D T` level pills | spacer | `12,438 lines` counter | `● LIVE` button.

**Stream (flex: 1):** scrollable log rows. Paused banner is `position: fixed` above the search row.

**Search row (38px):** full-width query input with `/` kbd hint on the right + `[: commands]` button to its right.

**Status bar (24px):** `● connected · ws://sift.local/v1/tail` · `12,438 records · 23/s` · `window 11:34 → 12:34` · right-aligned `press ? for keyboard shortcuts`.

**Left rail (40px, full height):** brand logotype (gradient-purple `s`) + 1 active icon (Logs) + spacer + Settings + Help (`?`). Alerts (M5) and Plugins (M4) are NOT shown in v1 — we don't have those features yet.

## 5. State model

Single top-level `AppState` struct lifted to the `App` component (the pattern T7 UX v2 already established for `TailCtx`, `LevelCtx`, `OpenDetail`). WebSocket is owned by the `App` shell, not individual routes.

```rust
pub struct AppState {
    // Data
    pub records: VecDeque<Record>,         // bounded ring, capacity 5000
    pub pending: Vec<Record>,              // buffered while paused
    pub pending_count: usize,              // for the banner
    pub status: Status,                    // connection status enum

    // View
    pub time_window: TimeWindow,           // enum: Preset(Duration) | Custom { from, to }
    pub level_set: LevelSet,               // 6 bools (mirror of sift-cli's LevelSet)
    pub timestamp_mode: TimestampMode,     // Absolute | Relative
    pub query: String,                     // native Leptos signal — no tui-input equivalent in wasm
    pub selected_row_index: Option<usize>, // index into `records` for inline expand (client-local)
    pub detail_expanded: bool,             // one row at a time

    // Live / paused
    pub live: bool,                        // true = streaming, false = paused
    pub at_bottom: bool,                   // tracked via scroll handler
    pub flash: Option<Flash>,              // (msg, kind, expiry)

    // Command palette
    pub command_open: bool,
    pub command_input: String,
    pub pending_command: Option<CommandAction>, // drained each tick
    pub help_open: bool,
}
```

Shape is deliberately similar to the TUI's `AppState` — same state methods (`push_record`, `toggle_pause`, `set_filter`, `open_detail`, `clear_records`, `save_records`, etc.) so the mental model carries over.

## 6. Data flow

### 6.1 WebSocket (live)

Same `/v1/tail` endpoint as the current UI. Connect on page load; auto-reconnect with exponential backoff (1/2/4/8/16/30s capped) — same logic as current `ws.rs` but with `at_bottom` + `live` gating.

On `record` frame:
- If `live && at_bottom`: push to `records`, evict oldest if `records.len() > 5000`, auto-scroll to bottom.
- If `!live`: push to `pending`, increment `pending_count`. Do NOT touch `records`.
- On resume: drain `pending` into `records` (evict as needed), clear `pending_count`, scroll to bottom.

### 6.2 Query API (windowed)

When the user selects a preset / custom range OR pauses, the UI fires `GET /v1/search?q=<query>&from=<iso>&to=<iso>&limit=500` to populate `records` from the backend. This replaces the current `/search` route's query logic.

While LIVE, the UI does NOT query the backend for history — records come only via WebSocket. The backend already retains all records (retention: permanent), so rewinding a bit is just a query.

While paused: the user can scroll up. Near the top of the current `records` slice, the UI fires a backward pagination query (`GET /v1/search?before=<oldest_ts>&limit=500`) to extend history backward. A loading spinner at the top indicates fetch-in-progress.

### 6.3 URL state (v1 minimal)

The URL stays at `/`. No encoded state. Refreshing loads the default view (Last 1h, LIVE on, all levels default, no query).

URL-encoded state (for shareable links) is deferred to v2.

### 6.4 Live ↔ paused transitions

| Trigger | Effect |
|---|---|
| Scroll up (any amount) | `live = false`, `at_bottom = false`. Pending buffer starts filling. |
| Click resume banner / press `G` / click `LIVE` | Drain pending → records; scroll to bottom; `live = true`; `at_bottom = true`. |
| Press `Space` | Toggle `live`. If transitioning to live, scroll to bottom + drain pending. |
| Select custom time range with explicit `to` | `live = false`; fire backend query for the range; replace records. |
| Select preset "Last 1h" while already live | Keep `live = true`; the window rolls with `now`. |
| Connection drops | `live = false`; `status = Reconnecting`; banner changes to "disconnected · retry now". |
| Connection recovers | Does NOT auto-resume live. User must click resume or press `G` (to avoid log-level jump surprises). |

## 7. Styling / theme

Single dark theme for v1. Palette sampled from the brainstorm `sift-v6.html`:

```
--bg:            #0e1014   (body)
--bg-2:          #14171d   (panels, rails)
--panel:         #1a1d24   (input fields, level pills when on)
--panel-2:       #20242c   (darker hover states)
--border:        #262a33
--border-focus:  #3a4050
--text:          #e5e7ec
--text-muted:    #858b98
--text-dim:      #555b68
--accent:        #a99bff   (brand purple, host names, selection)
--accent-bg:     rgba(169, 155, 255, 0.12)
--ok:            #6bd49b   (LIVE, info level, success flash)
--warn:          #ffb85c   (warn level, numbers in body)
--error:         #ff6b6b   (error level, error flash)
--fatal:         #ff4dbf   (fatal level)
--debug:         #6ab0ff   (debug level)
```

Typography:
- UI chrome: system sans (`ui-sans-serif, -apple-system, "Segoe UI", system-ui, sans-serif`)
- Logs + query input: monospace (`ui-monospace, "JetBrains Mono", "SF Mono", Menlo, Consolas, monospace`)

## 8. Dependencies

**No new crates from crates.io in v1.** The existing deps are sufficient: `leptos 0.6`, `leptos_router 0.6`, `gloo-net`, `gloo-timers`, `web-sys`, `wasm-bindgen`, `serde`, `serde_json`, `futures`. Input fields use plain `<input>` elements bound to Leptos signals — no `tui-input` equivalent needed in wasm.

**`jsonfmt` port:** the existing `crates/sift-cli/src/tui/jsonfmt.rs` module (pure Rust, `&serde_json::Value → Vec<Line<'static>>`) is ported to `ui/sift-ui/src/jsonfmt.rs` with its ratatui `Line`/`Span` types replaced by plain `Vec<JsonToken>` where `JsonToken` is a simple `(color: JsonColor, text: String)` pair. The Leptos `row_detail` component renders the tokens as styled `<span>` elements. No syntect dep (would bloat the wasm bundle by ~500KB for theme + syntax data — not worth it in v1).

**Dep-freshness note:** per CLAUDE.md, verify each dep is still at latest stable when the plan is written. Leptos 0.7 may have dropped by then — if so, the plan should include an upgrade step (0.6 → 0.7) as Task 1, analogous to the TUI overhaul's Ratatui bump. If Leptos 0.6 is still current, skip that task.

## 9. File structure (target)

```
ui/sift-ui/src/
├─ main.rs           # entry — unchanged
├─ app.rs            # shell: Router → single route (/)
├─ state.rs          # NEW: AppState + methods (mirror of sift-cli tui state)
├─ ws.rs             # existing: WebSocket connect + backoff — adapt to new state shape
├─ query.rs          # NEW: /v1/search fetch helper (backward pagination)
├─ command.rs        # NEW: parse + CommandAction enum (port from sift-cli)
├─ jsonfmt.rs        # NEW: port from sift-cli/src/tui/jsonfmt.rs (client-side JSON highlight)
├─ keys.rs           # NEW: KEYBIND_TABLE constant (single source of truth, mirror of sift-cli)
├─ routes/
│  └─ mod.rs         # collapse: old tail.rs + search.rs DELETED
├─ components/
│  ├─ rail.rs        # left rail (brand + icons)
│  ├─ top_bar.rs     # time range pill + level pills + line count + LIVE button
│  ├─ stream.rs      # the log stream with row rendering + inline expand
│  ├─ row.rs         # single row (grid columns)
│  ├─ row_detail.rs  # inline expand content (body + labels + fields + raw)
│  ├─ paused_banner.rs # floating resume pill
│  ├─ search_row.rs  # bottom search input + commands button
│  ├─ status_bar.rs  # connected/records/window/help hint
│  ├─ help_modal.rs  # ? overlay
│  └─ command_modal.rs # : palette overlay
└─ styles.css        # CSS vars, global styles (mirrors brainstorm palette)
```

**Files to delete:** `routes/tail.rs`, `routes/search.rs`. Their behavior migrates into `stream.rs` + `state.rs`.

**Files to adapt:** `ws.rs` keeps its backoff logic but its `on_record` handler becomes a simpler `state.apply_ws_event(evt)` (same contract as TUI).

## 10. Migration

Current `/tail` and `/search` routes are removed. Migration:

1. The router defines only `/` in v1; `/tail` and `/search` become 404s at the router level. The SPA fallback (`web.config`, shipped in UX v2) still rewrites all unmatched paths to `/index.html`, which means users hitting `/tail` or `/search` load the app and land on `/`.
2. **Bookmarked `/search?q=...` URLs:** on app mount, the shell inspects `window.location.pathname + search`; if it matches `/search?...`, parses `q`, `from`, `to` from the search params into the new state BEFORE the first render, then `history.replaceState(null, '', '/')` to normalize the URL. This preserves user intent from old bookmarks without requiring them to re-type.
3. `/tail` with no params: just lands on `/` in default state (LIVE, Last 1h). No data loss.

The `web.config` SPA fallback rule (already shipped in UX v2) covers deep-link refreshes automatically once the single route is in place.

## 11. Open items (to revisit post-v1)

- **Saved views panel** — left side panel with a list of saved queries. Requires a `/v1/views` backend (SQLite row: id, name, query, from, to, created_at).
- **Clickable timeline histogram** — top band or right panel. Interactive: click bar to set window, drag to zoom. Backend: `/v1/histogram?from=...&to=...&buckets=60` returning counts-per-bucket.
- **URL state** — `/?q=error&range=1h&live=false`. Enables shareable links.
- **Structured query parser** — `level:error app:api-prod duration>5s` parsed into filters. v1 is free-text only.
- **Light theme** — just a CSS var swap once the dark theme is baked.
- **Virtualized row rendering** — 5000 rows render fine in the DOM today, but at 50k+ we'd need windowed virtualization. Benchmark first.
- **Throbber for backward pagination** — tiny spinner at top of stream while `/v1/search?before=...` fetches.

## 12. Success criteria

v1 is "done" when:

1. `/` renders the unified stream; `/tail` and `/search` routes are gone.
2. On page load, LIVE is on, Last 1h is the window, records stream in.
3. Scroll up pauses live; resume banner appears; clicking it returns to bottom + live.
4. Changing time range fires `/v1/search` and replaces records; live off.
5. Level pills filter client-side.
6. `:` opens command palette; all 9 verbs dispatch correctly.
7. `?` opens help; same keybind table as TUI.
8. Row click expands inline detail with `jsonfmt`-highlighted `fields` (ported from `sift-cli/src/tui/jsonfmt.rs`).
9. Keyboard shortcuts match the TUI's (j/k/g/G/Enter/Space/I-W-E-F-D-T/Ctrl+T/`/`:/`?`).
10. `cargo build` for `sift-ui` (wasm32) passes clean; `trunk build --release` succeeds; deployed bundle works end-to-end with a real `sift-server`.

---
