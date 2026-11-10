# tracecure-ui — Leptos 0.8 CSR / wasm32

Loaded when working under `ui/tracecure-ui/`. Non-obvious things that have
cost real debugging time here.

## This crate is outside the cargo workspace

Root `Cargo.toml` excludes it (wasm32 target features are incompatible with
the server binaries). Consequences:

- `cargo test --workspace` from the repo root does **not** cover this crate.
  Run tests from inside `ui/tracecure-ui/`.
- `cargo clippy --target wasm32-unknown-unknown --all-targets` is the check
  that matters; the workspace clippy run never sees this code.
- `AppState` and everything in `src/state.rs`, `inbox.rs`, `command.rs`,
  `keys.rs`, `jsonfmt.rs`, `query.rs` is plain Rust in the **lib** target and
  is unit-testable on the host — `cargo test --lib`. Components and `ws.rs`
  are in the **bin** target and are not.

Keep logic in the lib so it stays testable off-wasm. That is why `AppState`
has no Leptos types in it.

## Reactivity: signal granularity is load-bearing, not style

`RwSignal::update` notifies **every** subscriber unconditionally — there is no
field-level diffing and no `PartialEq` gate. A single coarse
`RwSignal<AppState>` therefore means one arriving log record wakes every
closure in every mounted row.

That is why `Selection`, `ViewOpts` and the clock live in **separate
contexts** (`SelectionCtx`, `ViewCtx`, `ClockCtx` in `src/app.rs`) rather than
inside `AppState`. `components/row.rs` deliberately never reads `AppCtx`.
Before adding a signal read to a component that renders per-record, ask what
wakes it.

## The `<For>` key must be `Entry::seq`, never a positional index

`records` is a ring buffer capped at `RECORD_CAP`. At the cap, `push_record`
does `pop_front` + `push_back`, so a positional key set is **unchanged**
(`{0..CAP-1}`) even though every record shifted a slot. A keyed reconciler
correctly concludes there is nothing to patch, and the rendered rows silently
detach from the data while still costing full render work.

`Entry.seq` is a stable per-record identity that survives eviction. Selection
is keyed by it too, so it follows its record.

## Wall-clock rendering needs an explicit dependency

Anything whose output depends on `Utc::now()` rather than on state — relative
timestamps, the rolling histogram window — must `track()` `ClockCtx`, and
only on the branch that needs it (Leptos re-collects dependencies per run, so
an absolute-mode row takes no dependency). There is no ambient periodic
re-render to rely on.

## Do not scroll the container per batch

`scrollTop = scrollHeight` against a `content-visibility` container forces a
layout. Doing it per WS flush locked the main thread under load. See the note
on `AppState::push_batch`.

The tail is followed without that (details at the top of
`components/stream.rs`):

- **CSS scroll anchoring** does the per-record work. While `.stream` has the
  `following` class, `.stream-anchor` — the 1px sentinel after the last row —
  is the only element the browser may anchor to, so rows inserted above it
  scroll the view in the browser's own layout pass.
- A **1 Hz re-pin** covers estimate drift and browsers without anchoring.
- `scroll_request_counter` covers resume and a freshly loaded buffer.

`following` is only set while live. Paused, default anchoring applies, so the
row being read stays put when its detail pane opens.

**Pausing is driven by user intent, not scroll position.** The scroll handler
acts only inside a short window after a wheel, touch, scrollbar press or
scroll key. Do not go back to "not at the bottom ⇒ paused": a resize, a filter
changing the list height and a refetch all move the scroll position too, and
that rule paused and resumed the tail on its own.

## A root class must not share a name with a row cell

The shell is `.shell`, not `.app`, because rows have an `.app` cell and the
shared name applied the root's `width: 100vw; display: flex` to every one of
them — the app name spilled over the level column. Row cells are `.date`,
`.host`, `.app`, `.lvl`, `.body`; keep those names out of layout rules.

## Styling: where blur may and may not go

`styles.css` is the only stylesheet and its header states both rules; they are
repeated here because each one was learned by breaking it.

- **No `backdrop-filter` on `.stream`, `.row` or `.row-detail`.** The list
  holds thousands of rows and repaints on every arriving record. Blur belongs
  on small fixed panels only (rail, top-bar islands, histogram, the dock, the
  floating capsules, modals).
- **An island's blur sits on its `::before`**, not on the island. An element
  with `backdrop-filter` becomes the containing block for `position: fixed`
  descendants and a backdrop root for nested blurs — the range menu's
  click-away layer shrank to the island and the menu's own blur stopped
  working. Use the `.island` class rather than re-deriving the recipe.

Row columns are placed with `grid-template-areas`, so the visual order
(time → level → host → app → message) is independent of the markup order.
Severity is carried by the level tag and a faint row tint; do not recolour the
message text by level.

The floating pieces (the dock, the paused banner, the flash) are positioned
from the custom properties at the end of `:root` (`--status-h`, `--dock-h`,
`--dock-bottom`, `--rail-w`). Change a panel's size there, not in the rule
that happens to read it, or the capsules drift off the dock.

## The buffer is a window, and the UI must say so

`records` holds the newest slice of what the server matched — see
`AppState::loaded` (`text`, `limit`, `truncated`). Anything computed
client-side (the filter, level buttons, the histogram, the rate) is computed
over that slice. When adding such a feature, check it against a truncated
buffer and make the scope visible, the way the count ("shown of loaded"), the
status bar and the note above the oldest row do. Every explicit load goes
through `run_window_fetch`, which is generation-checked so a slow response
cannot overwrite a newer one.

## Verifying in a browser

`trunk serve`'s WebSocket proxy accepts the `/v1/tail` upgrade and then
forwards **no frames** — the UI backfills but sits on "connecting…" forever.
Use the server to serve the built assets instead:

```
trunk build --release
cargo run -p tracecure-server -- serve --data <tmp> --bind 127.0.0.1:7999 \
  --ui-dist ui/tracecure-ui/dist
```

**Measurement trap:** an occluded Chrome window reports
`document.visibilityState === "visible"` while starving `requestAnimationFrame`
(~2 frames/1.5 s) and throttling `setTimeout`. That looks identical to a frozen
renderer and has produced false conclusions here. Before trusting any perf
number, assert `document.hidden === false` **and** that a probe sees >20
frames per 1.5 s. Drive load from outside the browser.

## Bundle budget

CI fails if the gzipped `.wasm` exceeds 600 KB (currently ~247 KB). Note
`index.html` sets `data-wasm-opt="0"`, so trunk's `wasm-opt` pass is off; the
release profile (`opt-level = "z"`, `lto`, `codegen-units = 1`, `strip`)
carries the size instead.
