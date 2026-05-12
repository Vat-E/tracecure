# UI Polish Pass — Sizing, Tracecure Signature, App/Host Facets, Rate Histogram

**Status:** accepted design, awaiting implementation plan
**Date:** 2026-05-12
**Area:** UI (`ui/tracecure-ui/`)
**Milestone:** between M3 (text index) and M4 (plugin host) — UX work, not a milestone item, but unblocks usable FAN_iD integration.

## 1. Context

Two observed problems with the current UI (deployed via `.\deploy.ps1 -Ui`, viewed at the user's tracecure-server with `--ui-dist`):

1. **Chrome elements feel cramped at typical desktop DPI.** Search row is 38px, status bar 24px, body font 11–12px, kbd hints 10.5px. With 787+ records on screen the operator strains.
2. **The UI under-uses Tracecure's existing visual signature.** The palette already defines `--accent: #a99bff` (lavender) and `--accent-bg: rgba(169, 155, 255, 0.12)`, but those tokens are mostly absent from the rendered chrome — the LIVE pill is generic green, search focus is grey, the brand "t" in the rail is dim grey not lavender. Result: the UI reads as generic dark-tool, indistinguishable from Mezmo et al.

A separate workstream is needed for **app/host faceted filtering** — operators of FAN_iD want to focus on `fanid_app` or `fanid_admin` without typing `app:fanid_app` into the search every time. Today the only filter is level + free-text body substring. The records carry `host`, `app`, `stream_id`, `labels`, `fields` — all unused by the UI.

A third smaller add: a **30-bucket rate histogram** above the stream, tinted for warn/error spikes. Datadog/Loki convention; gives "is anything wrong" at a glance without scanning rows.

All three land in one polish pass to avoid two visual discontinuities.

## 2. Goal

Coordinated visual + interaction pass across the UI shell:

- **Sized for readability.** 13px body across the chrome, larger bars, lavender focus affordances.
- **Tracecure-distinct.** Lavender accent prominently used wherever the UI signals "active", "selected", or "primary" — not generic dark.
- **App + host filters.** Pills next to the level filter, with a "+ N more ▾" overflow popover that scales to dozens of values.
- **Rate histogram.** Client-side bucketed view of record rate, with level-tinted spikes.

**Non-goals (deliberately out of scope):**

- Saved views (`localStorage` named filter combos). Considered, deferred — solid follow-up if recurring filters become a pain point.
- Drag-select time histogram. Considered, deferred — overkill until incident-zooming is a routine activity.
- Light theme. Considered, deferred — single dark theme remains.
- Server-side filter API extension. All facet filters are **client-side** over already-loaded records. No `/v1/search` changes.
- Authentication, role-based scoping, multi-user views. M6 territory.
- Changes to the TUI (`tracecure-cli tui` subcommand). Has its own design history; not coupled to web UI polish.
- Modal redesigns (`help_modal`, `command_modal`, `row_detail` JSON popup). Will inherit the chrome palette updates passively but are not actively redesigned in this pass.
- Mobile / responsive layouts. Tracecure is a desktop ops tool.

## 3. Design language

### 3.1 Palette (extends existing tokens — no breaking changes)

Existing `:root` tokens stay; this pass uses them more deliberately and adds a few light variants:

| Token | Existing value | New role in this pass |
|---|---|---|
| `--accent` | `#a99bff` (lavender) | LIVE pill border+text; search focus border+ring; rail brand "t" color; active rail icon background; 2px inset on "new since paused" rows; "+ N more ▾" button text; popover checkbox checked state |
| `--accent-bg` | `rgba(169, 155, 255, 0.12)` | LIVE pill background; search focus ring (3px halo); active rail icon background; "+ N more" hover |
| `--ok` `#6bd49b` | unchanged | App-pill active color/tint; connected status dot |
| `--warn` `#ffb85c` | unchanged | Host-pill active color/tint (matches in-row host accent); warn level-pill tint; warn level text; warn histogram bucket bar |
| `--error` `#ff6b6b` | unchanged | Error level-pill tint; error level text; error histogram bucket bar |
| `--fatal` `#ff4dbf` | unchanged | Fatal level-pill tint |
| `--debug` `#6ab0ff` | unchanged | Info/debug level-pill tint (debug uses 0.7 opacity for dim) |
| New: `--accent-text-strong` | `#cdc4ff` | Active LIVE/popover text on lavender backgrounds |
| New: `--bg-deep` | `#0a0c0f` | Rail background and status-bar background — a "frame" shade darker than the stream/main `--bg`. |

Two new tokens added; everything else stays.

### 3.2 Type scale (chrome only — row content unchanged)

| Surface | Today | New |
|---|---|---|
| Body font (rows, search, top bar pills) | 11–12px | **13px** |
| Top-bar pills | 11px | **12px** |
| Level pills | 10–11px | **11px** (uppercase, weight 600, letter-spacing 0.04em) |
| App/host pills | n/a | **11.5px** (weight 400) |
| Kbd hints | 10.5px | **11.5px** |
| Status text | 11px | **12px** (values bold, labels dim) |
| Brand "t" in rail | 14px | **18px** (lavender) |

Monospace family stays everywhere — same `ui-monospace, "SF Mono", Menlo, Consolas, monospace` stack. Tabular-nums turned on for timestamp column (`font-variant-numeric: tabular-nums`).

### 3.3 Vertical rhythm (chrome only)

| Surface | Today height | New height |
|---|---|---|
| `.rail` | 40px wide | **48px wide** |
| `.rail` brand `.brand` slot | 22×22 | **28×28** |
| `.rail` icons | 26×26 | **32×32** |
| `.top` (filters row) | 38px | **52px** (single row with level pills) when only level + time filters; **52px + 36px = ~88px** (two rows: filters · then apps · hosts) when app/host filters are present |
| `.histogram-strip` (NEW) | — | **44px** (8px padding + 28px sparkline) |
| `.row-r` (log row) | 22px | **34px** |
| `.search-row` | 38px | **56px** |
| `.search` input box inside | 28px | **36px** |
| `.status` | 24px | **32px** |

All paddings move from `0 12px` to `0 16px` for chrome. Gaps between adjacent elements unify at `8–12px`.

### 3.4 Borders & shadows

- Borders soften: `--border` (`#262a33`) for most lines; `--bg-2` (`#1d2028`) for inter-bar separators (subtler).
- Search focus ring: 1px `--accent` border + 3px `--accent-bg` halo.
- Top bar gets a subtle linear gradient: `linear-gradient(180deg, var(--bg-2) 0%, var(--bg) 100%)` — adds depth, distinguishes from the stream below.
- LIVE pill dot gets a 6px lavender glow (`box-shadow: 0 0 6px rgba(169, 155, 255, 0.6)`).
- Connected status dot gets a 4px green glow.

## 4. Per-component changes

### 4.1 `rail.rs` — minor

```rust
// Brand "t" now lavender (uses --accent token via CSS).
<div class="brand">"t"</div>
// Active icon uses .icon.active styling (lavender + accent-bg).
```

CSS: `.rail` width 40→48px, `.brand` 22→28px and color shifts from dim grey to `--accent`, `.icon` 26→32px.

No Rust logic changes. Pure CSS + minor markup unchanged.

### 4.2 `top_bar.rs` — major rewrite

Today the top bar is one row: time pill · level pills · spacer · records count · LIVE pill.

New top bar is **a stacked container with two rows**:

```rust
<div class="top">
  <div class="row">
    {/* time + levels + counts + LIVE — same shape as today, restyled */}
  </div>
  <Show when=move || !state.with(|s| s.records.is_empty())>
    <div class="row">
      {/* apps + hosts filter pills + "+ N more" overflow */}
    </div>
  </Show>
</div>
```

The second row appears once *any* records have arrived (a record always carries both `app` and `host`, so the first record reveals one of each facet). Before any records, the second row is hidden — the top bar collapses to its single-row state during initial connect.

**Row 1** (always visible):

- Time-range pill (existing `<TimeRangeDropdown />` reskinned to pill style)
- 1px vertical divider (`.divider`)
- 6 level pills (`<LevelPills />` — tinted style, see §4.2.1)
- `.spacer`
- Records count: `<strong>{n}</strong> records`
- LIVE pill (new style — lavender border + bg, glowing dot)

**Row 2** (only rendered when `state.has_facets()`):

- `apps:` label
- Top-N app pills (default N=6, see §6 for selection)
- "+ K more ▾" button (only if there are >N apps; opens facet popover)
- 1px vertical divider
- `hosts:` label
- Top-N host pills
- "+ K more ▾" button (only if there are >N hosts)

### 4.2.1 Level pills — tinted style

Replaces the current single-letter `I W E F D T`. Each pill renders the full level name with its level color, tinted background when active, outline-only when off. Click toggles.

| Level | On color/tint | Off |
|---|---|---|
| INFO | `--debug` text + `rgba(106, 176, 255, 0.10)` bg + `rgba(106, 176, 255, 0.5)` border | dim grey + dim border |
| WARN | `--warn` text + `rgba(255, 184, 92, 0.10)` bg + … | … |
| ERROR | `--error` text + `rgba(255, 107, 107, 0.10)` bg + … | … |
| FATAL | `--fatal` text + `rgba(255, 77, 191, 0.10)` bg + … | … |
| DEBUG | `--debug` text at 0.85 opacity + same as INFO bg/border | … |
| TRACE | dim grey + transparent + dim border | (same off styling) |

Borders use `border-radius: 999px` (full pill). Hover lifts opacity by 0.1.

### 4.3 `app_host_filters.rs` — NEW component

Lives inside `top_bar` as a child or separate component file, depending on Leptos ergonomics. Responsibility: render the apps + hosts row, including the "+ N more ▾" button and the popover.

```rust
#[component]
pub fn AppHostFilters() -> impl IntoView { /* … */ }
```

**Facet derivation:**

The visible top-N pills come from a memo over `state.records`:

```rust
fn distinct_apps(records: &VecDeque<Record>) -> Vec<(String, usize)> {
    // (app_name, record_count) sorted by count desc, ties broken by name asc.
    // O(n) over records; n is bounded by RECORD_CAP=5000.
}
```

Two memos: `distinct_apps()` and `distinct_hosts()`. Recomputed on every record change. Cheap at 5000 records, ~5μs in WASM.

**Top-N selection:**

`N = 6` by default. The top-N pills come from `distinct_apps()[..6]`. If the source has ≤6 distinct apps, the "+ N more ▾" button is hidden.

**Pill state:**

Each pill displays the app name. The `on/off` styling comes from membership in `state.app_set: BTreeSet<String>`. Click toggles membership.

App pills tint green (`--ok` color and tint), matching the in-row `app` color. Host pills tint orange (`--warn` color and tint), matching in-row `host` color.

**Active filter logic:**

`AppState::filtered()` extends:

```rust
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

fn app_set_allows(&self, app: &str) -> bool {
    self.app_set.is_empty() || self.app_set.contains(app)
}
```

**Semantics:**

- Empty `app_set` = no app filter (show all). User intent: "I haven't filtered apps yet."
- Non-empty `app_set` = strict include-only-these-apps filter.

This contrasts with `level_set` which uses individual booleans per level. The difference is intentional: the level set is fixed (6 known levels, all present at compile time), while apps are discovered at runtime. An empty discovered-facet set should mean "no filter applied," not "filter everything out."

When a record arrives with an app not in `app_set` (and `app_set` is non-empty), the record is **filtered out** of the visible stream but still **counted in `records` and `pending`**. The facet pill for that app appears (with its count). If the user clicks to include it, the filter loosens and existing records become visible.

**Defaults:**

`app_set` and `host_set` are empty on initial mount. User opt-in for filtering.

### 4.3.1 Facet popover (`facet_popover.rs` — NEW component)

```rust
#[component]
pub fn FacetPopover(
    facet: Facet,                          // Apps | Hosts
    open: RwSignal<bool>,
    anchor: NodeRef<leptos::html::Button>, // for positioning
) -> impl IntoView { /* … */ }
```

Triggered by clicking the "+ N more ▾" button. Renders a floating box anchored to the trigger, 280px wide.

**Contents:**

```
┌────────────────────────────────────┐
│ [⌕]  filter apps…               | │
├────────────────────────────────────┤
│ [✓] fanid_app             3,124    │
│ [✓] fanid_admin           1,887    │
│ [ ] fanid_ui                412    │
│ [ ] wallet                  204    │
│ [ ] emailbuilder             98    │
│ [ ] roboticket               66    │
│ [ ] webhook_worker           38    │
│ …                                  │
├────────────────────────────────────┤
│ [ all ] [ none ] [ invert ]        │
└────────────────────────────────────┘
```

- Top: text input filters the list by substring (case-insensitive).
- List: scrollable, max height 280px. Each row = checkbox + name + count (record count in the current window).
- Footer: `all` (add all visible to set), `none` (clear set), `invert` (toggle each visible).

**Dismissal:**

Click outside, Escape key, or click the trigger again. Implemented via a global click listener registered on mount.

**Empty state:**

If `facet.values` is empty (no records yet), the popover renders "No values yet — waiting for records." with a dim color.

### 4.4 `rate_histogram.rs` — NEW component

A 44px-tall strip rendered between the top bar and the stream:

```rust
#[component]
pub fn RateHistogram() -> impl IntoView { /* … */ }
```

**Layout:**

```
┌─────────────────────────────────────────────────────┐
│ rate · 1h    [▓▓▒▒█▒▒▒▓██▒...30 bars...]   ~97/min │
└─────────────────────────────────────────────────────┘
```

- Left label: `rate · {time_window_short}` (e.g., "rate · 1h", "rate · 24h")
- Center: 30-bucket flex sparkline. Each bar is `flex: 1` so the strip fills the available width.
- Right: `~{rate}/min` (the average rate over the current window, computed as `records_in_window / window_minutes`).

**Bucket computation:**

Memo over `state.records` + `state.time_window`:

```rust
struct RateBuckets {
    buckets: [RateBucket; 30],  // fixed-size array
}

struct RateBucket {
    total: u32,
    warn_count: u32,
    error_count: u32,
}
```

Compute:

1. From `time_window`, derive `[from, to]` timestamps.
2. Divide `[from, to]` into 30 equal-width buckets.
3. Bucket every record in `state.records` whose `timestamp` is in range.
4. For each bucket, count totals + warn + error.

Cost: O(records) over the visible set, ~5000 records max (`RECORD_CAP`). Recompute on each record push and each `time_window` change. In WASM this is microseconds at FAN_iD-scale (~100/sec). No debouncing in v1; if recompute cost becomes noticeable under higher ingest rates, batch via a coalescing `request_animation_frame` (out of scope here).

**Visual encoding:**

- Bar height proportional to `bucket.total` / `max_bucket_total` (linear, not log — keeps spike visibility).
- If `bucket.error_count > 0`: render bar with `--error` color at 0.85 opacity.
- Else if `bucket.warn_count > 0`: `--warn` at 0.7 opacity.
- Else: `--accent` at 0.35 opacity (default lavender wash).

**Min-width per bar:** 3px (so they're clickable on hover even if rate is uniform).

**Future hook:** the strip is positioned and structured to support `D · drag-select time histogram` later (out of scope here). The 30-bucket choice keeps the visual approach forward-compatible.

### 4.5 `stream.rs` / `row.rs` — minor

CSS-only changes:

- `.row-r` height 22→34px.
- Body font 11→13px.
- Padding 0 12px → 0 16px.
- Existing `.row.new` (lavender inset shadow, see prior fix commit `3711c7e9`) stays as-is.

No Rust logic changes. Tests unchanged.

### 4.6 `search_row.rs` — moderate

Restyle only — no logic changes:

- `.search-row` height 38→56px, padding 0 12px → 0 16px.
- `.search` input box: 28→36px, border `1px #262a33` → `1px var(--accent)` when focused, with `0 0 0 3px var(--accent-bg)` halo.
- Border radius 4→8px (softer pill).
- `.cmd-btn` matches: 26→36px, radius 4→8px.
- `.search .ic` (the `⌕` glyph): `--text-dim` → `--accent`.

Markup unchanged; the focus state is toggled via `:focus-within` on `.search`.

### 4.7 `status_bar.rs` — moderate

- Height 24→32px.
- Font 11→12px.
- Values rendered bold (`<strong>787</strong> records`), labels stay dim.
- Background `--bg-2` → `--bg-deep` (`#0a0c0f`) — same frame shade as the rail; reads as a darker boundary at the bottom of the shell.
- The connected status dot grows 5→6px and gains a 4px green glow (matches LIVE pill treatment).

### 4.8 `paused_banner.rs` — minor

Passive: pulls updated `--accent` palette automatically. Spacing tweaks to match the new 13px rhythm. No structural change.

## 5. State model changes

`AppState` (`ui/tracecure-ui/src/state.rs`) gains two fields:

```rust
pub struct AppState {
    // … existing fields …

    /// Visible apps. Empty set = no app filter applied.
    /// Non-empty = strict include-only-these-apps.
    pub app_set: BTreeSet<String>,

    /// Visible hosts. Same semantics as app_set.
    pub host_set: BTreeSet<String>,
}
```

Initialized to `BTreeSet::new()` in `AppState::new()`.

### 5.1 Mutations

```rust
impl AppState {
    pub fn toggle_app(&mut self, app: &str) {
        if !self.app_set.remove(app) {
            self.app_set.insert(app.to_string());
        }
    }

    pub fn toggle_host(&mut self, host: &str) {
        if !self.host_set.remove(host) {
            self.host_set.insert(host.to_string());
        }
    }

    pub fn set_apps(&mut self, apps: impl IntoIterator<Item = String>) {
        self.app_set = apps.into_iter().collect();
    }

    pub fn set_hosts(&mut self, hosts: impl IntoIterator<Item = String>) {
        self.host_set = hosts.into_iter().collect();
    }
}
```

The bulk setters cover the popover's `all` / `none` / `invert` footer actions.

### 5.2 `filtered()` extension

See §4.3 — adds two filter clauses (`app_set_allows`, `host_set_allows`).

### 5.3 Backwards compatibility

No serialization changes (state is in-memory only). No `localStorage` keys (saved views deferred). No URL params. Existing 12 tests in `state::tests` continue to pass unchanged.

## 6. Default settings

| Knob | Default | Notes |
|---|---|---|
| Top-N app/host pills inline | **6** | Above 6, overflow into popover. |
| Histogram buckets | **30** | Constant. |
| Sort order for inline pills | by record count desc, then name asc | Most active first. |
| Popover sort order | same | Consistent. |
| `app_set` / `host_set` initial state | empty (no filter) | User opts in by clicking a pill. |
| RECORD_CAP | unchanged (5000) | |

## 7. Files affected

### New
- `ui/tracecure-ui/src/components/app_host_filters.rs`
- `ui/tracecure-ui/src/components/facet_popover.rs`
- `ui/tracecure-ui/src/components/rate_histogram.rs`

### Modified
- `ui/tracecure-ui/styles.css` — extensive chrome restyling, new utility classes for `.app-pill`, `.host-pill`, `.lvl-pill`, `.histogram-strip`, `.facet-popover`, `.more-btn`.
- `ui/tracecure-ui/src/state.rs` — new fields, new methods, extended `filtered()`.
- `ui/tracecure-ui/src/components/mod.rs` — register new components.
- `ui/tracecure-ui/src/components/top_bar.rs` — rewrite for two-row layout, integrate level-pill restyle and `<AppHostFilters />` child.
- `ui/tracecure-ui/src/components/search_row.rs` — restyle.
- `ui/tracecure-ui/src/components/status_bar.rs` — restyle + bold values.
- `ui/tracecure-ui/src/components/stream.rs` — no logic, CSS class touches if needed.
- `ui/tracecure-ui/src/components/row.rs` — CSS only.
- `ui/tracecure-ui/src/components/rail.rs` — CSS only.
- `ui/tracecure-ui/src/components/paused_banner.rs` — spacing tweaks.
- `ui/tracecure-ui/src/app.rs` — mount `<RateHistogram />` between `<TopBar />` and `<Stream />`.

### Untouched
- `ui/tracecure-ui/src/ws.rs`, `query.rs`, `keys.rs`, `command.rs`, `jsonfmt.rs` — unchanged.
- `ui/tracecure-ui/src/components/command_modal.rs`, `help_modal.rs`, `row_detail.rs`, `flash.rs` — inherit palette passively; no active redesign.
- All backend crates (`tracecure-server`, `tracecure-storage`, `tracecure-cli`, `tracecure-core`, `tracecure-api`) — zero changes. This is UI-only.

## 8. Testing

### Unit tests (`state::tests`)

New tests:

- `app_set_allows_returns_true_when_set_is_empty`
- `app_set_allows_returns_true_when_app_in_set`
- `app_set_allows_returns_false_when_app_not_in_set`
- `toggle_app_inserts_then_removes`
- `set_apps_replaces_existing_set`
- `filtered_combines_level_app_host_query`
- Same shape for hosts.

Coverage target: every new public method has a test. Existing 44 tests continue to pass.

### Manual / visual verification

After deploy via `.\deploy.ps1 -Ui`:

1. Open the UI; confirm top bar shows tinted level pills (not letters).
2. With at least 2 distinct apps in the loaded records, confirm app pills auto-populate in the second row.
3. Click a pill → records from other apps disappear from the stream; pill turns to outline-only.
4. Click "+ N more ▾" when there are >6 apps → popover opens with full list + counts.
5. Type into popover search → list narrows.
6. Click "all" / "none" / "invert" → filter set updates accordingly.
7. Confirm rate histogram bars are visible above the stream; warn/error spikes tinted.
8. Resize window → histogram bars `flex: 1` and redistribute.
9. Trigger a search via `/` → search input gets lavender focus ring.
10. Confirm status bar shows bold record count + dim labels.

### Out of scope for tests

- WASM browser-driven tests (`wasm-bindgen-test`) — not used elsewhere in this crate; not adding for this pass.
- Performance benchmarks for facet derivation — single-pass O(n) over 5000 records is trivially fast in WASM.

## 9. Migration / rollout

This is a UI-only change. Deploy via `.\deploy.ps1 -Ui` (re-runs `trunk build --release` and updates the served `dist/`). No backend deploy required, no data migration, no compatibility shim. The change is visible immediately on next browser refresh.

If a user has the page open during the deploy, hitting refresh picks up the new bundle; their in-memory state (filter selections, etc.) resets — same behavior as today's UI on any update.

## 10. Decisions made during brainstorming (record for posterity)

- **Direction:** Tracecure character via existing lavender accent, not generic dark. Three exploratory mockups (Refined Mezmo / Linear-y modern / Datadog-y dense pro) all rejected — the user wanted *Mezmo-genre, not Mezmo copy*. Lavender (`#a99bff`) is Tracecure's signature; lean into it.
- **Level filter style:** option A · tinted pills (full names, color = level, tint when active). Rejected: B · color dot + label, C · swatch bar + label.
- **Additions:** A (app/source filter) + B (rate histogram). Rejected: C (saved views), D (drag-select time histogram). Both reasonable follow-ups.
- **Facet overflow strategy:** A · top-N pills + "+ N more ▾" popover. Rejected: B · single dropdown trigger per facet. A keeps frequently-used facets one-click; the long tail is one extra click.
- **Empty `app_set` semantics:** no filter (show all). Not "show none." Discovered facets must default to inclusive.

## 11. Open questions

None. The brainstorming pass closed every visual + interaction decision.

## 12. References

- Brainstorm artifacts: `.superpowers/brainstorm/3328-1778591429/content/` (mockups: `redesign.html`, `level-filter.html`, `proposed-v2.html`, `additions.html`, `overflow.html`, `final.html`).
- Current UI source: `ui/tracecure-ui/src/` (Leptos 0.8 CSR).
- Current palette: `ui/tracecure-ui/styles.css` `:root` (lines 2–17).
- Prior UI bug fix that introduced the "new since paused" lavender inset shadow: commit `3711c7e9`.
- Related architecture doc (currently stale on M0 → M1 → UI v2 → rename — needs separate refresh per the 2026-05-11 vault hygiene audit): `Vault/Sift/10-Architecture/UI/UI Overview.md`.
