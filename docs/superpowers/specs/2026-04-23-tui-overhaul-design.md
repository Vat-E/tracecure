# TUI overhaul — theme, panes, blocks, command mode

**Status:** accepted design, awaiting implementation plan
**Date:** 2026-04-23
**Applies to:** `crates/sift-cli/src/tui/` (Ratatui TUI for `sift tui`)
**Builds on:** UX v2 (search, detail, levels, pause, help, reconnect, JSON highlighting)

## Goal

Turn the TUI from a plain-text log view into a properly composed Ratatui app with visual hierarchy, focus-aware borders, a `:` command palette, and visibly active chrome (rate sparkline, record-count meta, level block colors, throbber during inflight searches). Feature parity with UX v2 is preserved; this is a visual + ergonomic overhaul, not a capabilities change.

## References

- **openapi-tui** (github.com/zaghaghi/openapi-tui) — focus-aware `BorderType::Thick` vs `Plain`, three-slot block titles, colored method chips, `:` command palette with Normal/Insert/Command modes and `[N]/[I]/[C]` badge, timed flash messages.
- **yozefu** (github.com/MAIF/yozefu) — `Theme` struct with named color slots, `syntect` for body syntax highlighting, `tui-input` for cursor-aware text inputs, `throbber-widgets-tui` for inflight spinners, `timeago` for relative timestamps, single-main-pane layout (no pane sprawl).
- **lazygit** — visual anchor for "structured, keyboard-driven TUI" that we want to meaningfully exceed rather than match. We're more considered (rounded corners, thinner chrome) and less dashboard-like.

## Guiding principles

1. **Widget-first.** Every visible region is a `Block`. Every text input uses `tui-input`. Every list of structured rows uses `Table`, not `List`. No hand-rolled space-padded strings.
2. **Focus is a first-class app state.** A `Pane` enum in `AppState` names the currently-focused region. Rendering reads `state.focus_pane` to pick border thickness and color.
3. **No nerd-font dependency.** Base Unicode (`╭ ╮ ╰ ╯ ─ │ ┃ ● ○ ▕ ▸ ⟨ ⟩ · ≡ … ■ ░ ▁▂▃▄▅▆▇█`) only. The TUI works over plain SSH into a bare-bones terminal without extra fonts.
4. **One coherent dark theme** in a struct, no per-user theming for now. Future config-driven theming slots in cleanly when needed.
5. **Command mode is a real feature**, not a TODO. `:pause`, `:save path=…`, `:search q=… from=… to=…`, `:level debug on` must all parse, dispatch, and flash a result in the footer.
6. **Latest-stable pinning** per the project's dep-freshness rule: the spec pins versions as of 2026-04-23.

## Non-goals (deliberately deferred)

- **Multi-pane side-by-side layout** (à la yozefu's topics list + records table). Sift has no obvious left-sidebar primitive today — maybe "saved searches" when that lands, post-M1. Stay single-main-pane.
- **Mouse support.** Terminal-first, keyboard-only. Clicking isn't wired.
- **Pixel-style rendering** (Sixel, Kitty graphics protocol). Sparkline only; no record-volume histogram in the TUI.
- **Custom syntect theme selection.** Bundled default is fine.
- **Command history** (`Up` in command prompt to recall last command). Nice-to-have; defer.
- **Autocomplete UI** for commands (ghost-text preview, dropdown). We ship basic tab-completion; visible autocomplete popup can come later.
- **Config file** for theme, keybinds. All hardcoded in the theme struct and input dispatcher.
- **Changing any server-side behaviour.** All endpoints, frames, and record shapes used today remain.

## Dependencies

New additions (pinned at current latest stable as of 2026-04-23):

```toml
ratatui = "0.30"          # up from 0.29 — matches both references; ratatui-crossterm split
crossterm = "0.29"        # up from 0.28
tui-input = "0.15"        # cursor-aware input widget
syntect = "5.3"            # syntax highlighting for detail-popup JSON
throbber-widgets-tui = "0.11"  # spinner during inflight search
timeago = "0.6"            # relative-time formatting
```

Ratatui 0.30 split its terminal backend into `ratatui-crossterm`, gated by feature flags. We enable `ratatui = { version = "0.30", features = ["crossterm"] }` so the matching `crossterm 0.29` is pulled through transitively. `tui-input 0.15.3+` already tracks ratatui 0.30 cleanly.

Expected API drift from the 0.29 → 0.30 bump (discovered at implementation time):
- `Block::borders(Borders::ALL)` / `.border_type(BorderType::Rounded)` likely unchanged.
- `Layout::default().direction(...).constraints(...)` may have been fully replaced by the `Layout::vertical([…])` / `Layout::horizontal([…])` builders we already use post-0.29.
- `Tabs` and `Table` widget APIs are stable across 0.29/0.30.
- `Frame::render_widget` unchanged.

## Architecture

New module layout under `crates/sift-cli/src/tui/`:

```
src/tui/
├─ mod.rs               # lifecycle + event loop, largely unchanged
├─ theme.rs             # NEW — Theme struct + dark default; single source of colors
├─ focus.rs             # NEW — Pane enum + cycle logic
├─ command.rs           # NEW — command parser + dispatcher ("palette")
├─ state.rs             # grows: Pane focus, command prompt state, flash messages,
│                       #        rate history ring (60 samples), abs/rel toggle
├─ input.rs             # modal dispatcher: Normal / Insert / Command
├─ view.rs              # top-level draw dispatcher (unchanged in role)
├─ view_tail.rs         # REWRITTEN — Block everywhere, Table, Sparkline, tri-bar
├─ view_search.rs       # REWRITTEN — bordered inputs + throbber + Table
├─ view_detail.rs       # REWRITTEN — nested bordered sections, syntect body
├─ view_help.rs         # REWRITTEN — two-column key/description grid
├─ view_footer.rs       # NEW — one-row footer: hints OR command prompt + mode badge
├─ jsonfmt.rs           # KEPT — fallback JSON renderer, used when syntect fails
├─ record_input.rs      # NEW — thin wrapper around tui-input::Input for our text fields
└─ ws.rs                # unchanged
```

### `theme.rs`

```rust
use ratatui::style::Color;

#[derive(Clone, Debug)]
pub struct Theme {
    // Base
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,

    // Borders
    pub border: Color,           // inactive panes — dim
    pub border_focused: Color,   // active pane — accent color

    // Selection
    pub bg_selected: Color,
    pub fg_selected: Color,
    pub bg_selected_unfocused: Color,  // dimmer when selection is in an unfocused pane

    // Log levels
    pub level_trace: Color,
    pub level_debug: Color,
    pub level_info: Color,
    pub level_warn: Color,
    pub level_error: Color,
    pub level_fatal: Color,

    // Semantic
    pub shortcut_key: Color,    // the letter in "/ filter"
    pub shortcut_label: Color,  // the "filter" text
    pub flash_ok: Color,
    pub flash_err: Color,
    pub disabled: Color,

    // Syntect
    pub highlighter_theme: &'static str,  // e.g. "base16-ocean.dark"
}

impl Theme {
    pub fn dark() -> Self { /* default palette — see design §Palette */ }
    pub fn level_color(&self, lvl: sift_core::Level) -> Color { /* dispatch */ }
}
```

`Theme::dark()` is the only impl at launch. Stored in `AppState::theme: Theme` (not a `RwLock`; a thread-local clone is fine because the TUI has a single render loop).

### Palette (dark default)

| slot | color | notes |
|---|---|---|
| `fg` | `Color::Rgb(215, 215, 215)` (≈ `#d7d7d7`) | main text |
| `muted` | `Color::Rgb(120, 120, 120)` | timestamps, hints, inactive borders |
| `accent` | `Color::Cyan` | active borders, section titles |
| `border` | `Color::Rgb(60, 60, 60)` | unfocused blocks |
| `border_focused` | `Color::Cyan` | focused block |
| `bg_selected` | `Color::Rgb(50, 50, 80)` | selected row in focused Table |
| `fg_selected` | `Color::Rgb(255, 255, 255)` | selected-row text |
| `bg_selected_unfocused` | `Color::Rgb(40, 40, 50)` | same row when pane not focused |
| `level_trace` | `Color::DarkGray` | |
| `level_debug` | `Color::Cyan` | |
| `level_info` | `Color::Green` | |
| `level_warn` | `Color::Yellow` | |
| `level_error` | `Color::Red` | |
| `level_fatal` | `Color::Magenta` | |
| `shortcut_key` | `Color::Cyan` | the `/` in `/ filter` |
| `shortcut_label` | `Color::Gray` | the `filter` in `/ filter` |
| `flash_ok` | `Color::Green` | `:save wrote 342 records…` |
| `flash_err` | `Color::Red` | `bad command: unknown key \"foo\"` |
| `disabled` | `Color::DarkGray` | greyed-out levels etc. |
| `highlighter_theme` | `"base16-ocean.dark"` | bundled with `syntect::SyntaxSet::load_defaults_newlines()` |

### `focus.rs`

```rust
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Pane {
    Records,       // tail records Table
    Filter,        // tail filter input
    LevelsBar,     // tail/search level pills row
    RateBar,       // tail rate sparkline (non-focusable — skip in cycle)
    SearchQuery,   // search query input
    SearchFrom,    // search from input
    SearchTo,      // search to input
    SearchResults, // search results Table
    DetailPopup,   // detail popup body (scrollable)
    HelpPopup,     // help overlay
    CommandPrompt, // `:` command input
}
```

Focus cycling (`Tab` / `Shift+Tab`):

- **Tail mode:** `Records → Filter → LevelsBar → Records …` (skips `RateBar` because it's pure display).
- **Search mode:** `SearchQuery → SearchFrom → SearchTo → SearchResults → LevelsBar → SearchQuery …`.
- **Detail popup open:** focus locks on `DetailPopup`; Esc returns to previous.
- **Help popup open:** focus locks on `HelpPopup`; Esc returns.
- **Command prompt open:** focus locks on `CommandPrompt`; Esc cancels and returns.

### `command.rs`

```rust
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
    Level { level: sift_core::Level, on: bool },
    Help,
    Quit,
}

pub fn parse(input: &str) -> Result<CommandAction, CommandError> { /* … */ }

/// Tab-completion for a partial input. Returns the longest shared prefix of
/// matching commands, plus the match list for UI display.
pub fn complete(partial: &str) -> Completion { /* … */ }
```

Grammar:

```
command    ::= ':' verb (' ' kv)*
verb       ::= 'pause' | 'resume' | 'clear' | 'retry' | 'save'
             | 'search' | 'level' | 'help' | 'quit'
kv         ::= key '=' value
key        ::= [a-z][a-z0-9_]*
value      ::= quoted | bare
quoted     ::= '"' .* '"'
bare       ::= [^ "=]+
```

Verb arguments:

| verb | valid keys | example |
|---|---|---|
| `pause` | — | `:pause` |
| `resume` | — | `:resume` |
| `clear` | — | `:clear` |
| `retry` | — | `:retry` |
| `save` | `path` | `:save path=records.ndjson` |
| `search` | `q`, `from`, `to` | `:search q=error from=2026-04-23T00:00:00Z` |
| `level` | positional `<level>`, then `on` or `off` | `:level debug on` |
| `help` | — | `:help` |
| `quit` | — | `:quit` |

Validation:
- Unknown verbs → `CommandError::UnknownVerb`
- Unknown keys for a verb → `CommandError::UnknownKey`
- Malformed time in `from`/`to` → `CommandError::BadTime`
- Empty input → treated as no-op (Esc-equivalent)

On successful dispatch, a flash message lands in the footer:
- `:pause` → `paused — 0 queued`
- `:save path=records.ndjson` → `saved 342 records to records.ndjson`
- Error → `bad command: unknown verb "foo"` in `flash_err` color

Flash messages live 2s, then revert to keybind hints.

### `state.rs` additions

```rust
pub struct AppState {
    // existing… (records, search_*, paused, detail, level_set, help_open, filter, …)

    pub theme: Theme,
    pub focus_pane: Pane,

    pub command_open: bool,
    pub command_input: Input,          // tui-input::Input
    pub flash: Option<Flash>,          // Some(Flash) expires on next tick after show_until

    pub rate_history: VecDeque<u32>,   // 60 samples, one per second; current-second at end
    pub rate_second_start: Instant,    // when the current bucket opened
    pub rate_current: u32,             // records counted in the open bucket

    pub timestamp_mode: TimestampMode, // Absolute | Relative
}

#[derive(Debug, Clone)]
pub struct Flash {
    pub msg: String,
    pub kind: FlashKind,   // Ok | Err
    pub show_until: Instant,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TimestampMode { Absolute, Relative }
```

Rate bucket:
- On each `WsEvent::Frame(Record)`, `rate_current += 1`.
- A 1-Hz tick in the event loop checks: if `Instant::now() - rate_second_start >= 1s`, push `rate_current` onto `rate_history`, drop-front if len > 60, set `rate_current = 0`, update `rate_second_start`.
- Displayed sum: `rate_history.iter().sum::<u32>()` / 60 = avg rec/s over last minute. Displayed as integer `12/s` next to the sparkline.

Input wrapper:
- `command_input: tui_input::Input` — always present; the `command_open` bool gates whether it's rendered.
- Filter input, search q/from/to — switch from raw `String` fields to `tui_input::Input` via `record_input::InputState` (our thin wrapper that keeps the cursor and handles paste via crossterm).

### `input.rs` modes

```rust
pub enum ModalState {
    Normal,   // keybinds dispatch; no text entry
    Insert,   // a text input has focus; keystrokes append to it
    Command,  // `:` prompt active; keystrokes append to command_input
}
```

Dispatch precedence (in order):
1. Ctrl+C → quit (every mode).
2. If help popup open — Esc or `?` toggles it, else swallow.
3. If detail popup open — Esc closes it, else swallow.
4. If command prompt open (`command_open`) — typed chars go to `command_input`; Enter parses + dispatches; Esc cancels; Tab triggers `complete()`.
5. If focus is a text input (Filter, SearchQuery, SearchFrom, SearchTo) — typed chars go to that input; Esc or Enter exits Insert.
6. Otherwise — Normal-mode dispatch (current keybinds).

Mode badge in footer right:
- `[N]` Normal
- `[I]` Insert (any text input focused)
- `[C]` Command (`:` prompt open)

### Keybind map

**Global (any mode):**

| key | action |
|---|---|
| `Ctrl+C` | quit |
| `?` | toggle help (if not in Insert/Command) |
| `Esc` | close overlay → return to previous focus |
| `:` | open command prompt (if not in Insert/Command) |

**Tail mode, `Records` focus:**

| key | action |
|---|---|
| `j` / `↓` | cursor down |
| `k` / `↑` | cursor up |
| `g` / `Home` | jump to top |
| `G` / `End` | follow tail |
| `PgDn` / `PgUp` | scroll 10 |
| `↵` | open detail for cursor row |
| `Space` | pause/resume |
| `T` | toggle abs/rel timestamps |
| `I/W/E/F/D/T` | — wait, conflict with `T` for timestamp |

**Keybind collision: `T` is used for both "toggle Trace visibility" (from UX v2) and "toggle abs/rel timestamps" (new).** Resolution: move the timestamp toggle to lowercase `t`. Uppercase `T` stays as "toggle Trace visibility". Lowercase `t` also currently switches to Tail mode (from Search). Revised collision-free mapping:

| key | action |
|---|---|
| lowercase `t` | switch to Tail mode (unchanged from UX v2); no-op when already in Tail |
| uppercase `T` | toggle Trace level visibility |
| `Ctrl+T` | toggle absolute/relative timestamps |

(`Ctrl+T` is unused today and unambiguous. Documented in help.)

**Tail mode, `Filter` focus:**

| key | action |
|---|---|
| any printable | append to filter |
| `Backspace` | delete last char |
| `Esc` / `↵` | exit filter focus → `Records` |

**Tail mode, `LevelsBar` focus:**

| key | action |
|---|---|
| `←` `→` / `h` `l` | move between level pills |
| `Space` / `↵` | toggle the pill under the cursor |

(When a level pill is highlighted, pressing `I/W/E/F/D/T` keys still works — the `h/l` pill cursor is a secondary convenience for keyboards without shift access.)

**Search mode:** same as UX v2 plus `Tab` cycles through `SearchQuery → SearchFrom → SearchTo → SearchResults → LevelsBar`.

**Command mode:** Enter → dispatch, Esc → close, Tab → complete.

## Visual design

### Outer frame (every mode)

One rounded-border `Block` wrapping everything except the footer. Title is the tab strip:

```
╭─ sift  •  ⟨ Tail ⟩ · Search ─────────── 342/500  •  12/s ──╮
```

Construction:
- `Block::default().borders(Borders::ALL).border_type(BorderType::Rounded)`
- `.title(Line::from([Span::raw(" sift  •  "), span("⟨ Tail ⟩", accent+bold), Span::raw(" · "), span("Search", muted), Span::raw(" ")]))` — top-left
- `.title(Line::from(…))` with `.alignment(Alignment::Right)` for buffer/rate — top-right

Ratatui 0.30 `Block::title` takes a `Title` which can carry alignment; multiple titles coexist.

### Tail mode body

Vertical split inside the outer block:

```
┌ [Records Table — fills] ─────────────────────────┐
│  time          host       app    lvl   body      │
│  ───────────────────────────────────────────────  │
│  12:34:56.789  web-01     api    ░░░   request … │
│▕ 12:34:57.012  web-02     api    ███   db query …│
│  …                                                │
├─ filter ──┬─ levels ───────────┬─ rate 1m ───────┤
│ ▸ /error  │ ●I ●W ●E ●F ○D ○T │ ▁▂▃▅▇▅▃▂▁ 12/s │
└───────────┴─────────────────────┴─────────────────┘
```

Three sub-blocks inside a `Layout::vertical([Constraint::Min(1), Constraint::Length(3)])`:
- Records (fills)
- Tri-section middle bar (fixed 3 rows including borders)

Middle bar is itself a `Layout::horizontal([Constraint::Ratio(1,3), Constraint::Ratio(1,3), Constraint::Ratio(1,3)])` of three `Block`s with `.title(…)` for filter / levels / rate.

**Records Table:**

```rust
Table::new(rows, widths)
    .header(Row::new(["time", "host", "app", "lvl", "body"])
        .style(muted).bottom_margin(1))
    .row_highlight_style(bg_selected on focused, bg_selected_unfocused otherwise)
    .highlight_symbol("▕ ")
```

Column widths: `Constraint::Length(12)` for time, `Length(10)` for host, `Length(7)` for app, `Length(5)` for level, `Min(20)` for body. `body` truncates with `…` at column edge.

Time column: `HH:MM:SS.sss` absolute, or "Ns ago" / "Nm ago" relative, driven by `state.timestamp_mode` (via `timeago::Formatter`).

Level column: `Color::* blocks` — three half-block characters colored by level:

| level | cell | color |
|---|---|---|
| Trace | `░░░` | `level_trace` |
| Debug | `▒▒▒` | `level_debug` |
| Info  | `███` on `level_info` bg | reverse-video feel |
| Warn  | `███` on `level_warn` bg | |
| Error | `███` on `level_error` bg | |
| Fatal | `███` on `level_fatal` bg | |

(Sticking to safe Unicode only; no emoji.)

**Filter sub-block:**

```
┌─ filter ──┐
│ ▸ /error  │
└───────────┘
```

- `▸` prefix shown when focused; space when not.
- Input renders via `tui-input` — cursor position is tracked, arrow keys move cursor within the string, Ctrl+A/E for home/end.
- Border thick + accent-color when focused; plain + muted otherwise.

**Levels sub-block:**

```
┌─ levels ─────────────┐
│ ●I ●W ●E ●F  ○D ○T  │
└───────────────────────┘
```

- `●` for on, `○` for off.
- Letter color: `level_*` when on, `muted` when off.
- In `LevelsBar` focus, the currently-highlighted pill gets `bg_selected` bg; `h/l` moves the highlight.

**Rate sub-block:**

```
┌─ rate 1m ──────────┐
│ ▁▂▃▅▇▅▃▂▁ 12/s    │
└─────────────────────┘
```

- `Sparkline::default().data(&state.rate_history).max(<auto>)`.
- Trailing `N/s` = 60-sec average as integer.

**Footer (outside the outer block):**

```
 / filter · Space pause · ↑↓ nav · ↵ detail · s search · : cmd · ? help    [N]
```

- Single row, no border.
- Left: either keybind hints (context-sensitive per mode + focus) OR command prompt if `command_open`.
- Right: mode badge `[N]` / `[I]` / `[C]` in accent color.

When a `Flash` is active, replaces the hints:

```
 saved 342 records to records.ndjson                                        [N]
```

with `flash_ok` or `flash_err` color for the message.

### Search mode body

Replaces the Records/Middle stack with:

```
┌ [search header]                                                        ┐
│                                                                         │
│  ┌─ query ──────────────────────────────────────────────────────────┐  │
│  │ ▸ error                                                          │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                                         │
│  ┌─ from ────────────────────┐  ┌─ to ────────────────────────┐        │
│  │ 2026-04-23T12:00:00Z      │  │ 2026-04-23T13:00:00Z        │        │
│  └───────────────────────────┘  └─────────────────────────────┘        │
│                                                                         │
│  [⣾ searching…]  or  42 results · scanned 10,482 · 310 ms              │
│                                                                         │
│  time          host       app    lvl   body                            │
│  ───────────────────────────────────────────────────────────────────   │
│▕ 12:34:56.789  web-01     api    ███   request failed with timeout    │
│  …                                                                     │
│                                                                         │
├─ levels ──────────────────────────────────────────────────────────────┤
│ ●I ●W ●E ●F  ○D ○T                                                    │
└───────────────────────────────────────────────────────────────────────┘
```

- Each of query / from / to is its own bordered `Block` with `tui-input` content.
- Focused one has thick accent border; others plain muted.
- Inflight: `throbber-widgets-tui` renders `⣾ searching…` next to the status line.
- Results in a `Table` widget identical in shape to tail's.
- Levels bar persists (no rate bar — search is not live).

### Detail popup

80% × 80% centered, with `Clear` underneath. Outer block with title:

```
╭─ record · 12:34:56.789 · web-01 · api · ERROR ─────────────── × ─╮
```

Four nested bordered sections inside:

```
│ ┌─ body ──────────────────────────────────────────────────────┐ │
│ │ Connection to postgres timed out after 30s. Falling back …  │ │
│ └──────────────────────────────────────────────────────────────┘ │
│ ┌─ labels ────────────────────────────────────────────────────┐ │
│ │ env       prod                                              │ │
│ │ service   api                                                │ │
│ └──────────────────────────────────────────────────────────────┘ │
│ ┌─ fields ────────────────────────────────────────────────────┐ │
│ │ {                                                            │ │
│ │   "trace_id": "abc123",                                      │ │
│ │   "user_id": 42                                              │ │
│ │ }                                                            │ │
│ └──────────────────────────────────────────────────────────────┘ │
│ ┌─ raw ───────────────────────────────────────────────────────┐ │
│ │ (none)                                                      │ │
│ └──────────────────────────────────────────────────────────────┘ │
```

JSON in the `fields` section renders via `syntect::easy::HighlightLines` over the serialized JSON, with the configured `highlighter_theme`. Terminal color attributes come from `syntect::util::as_24_bit_terminal_escaped` or equivalent Ratatui adapter. If syntect fails (syntax not found, etc.), fall back to existing `jsonfmt::highlight` — no panic.

Scrollable: Up/Down scrolls the popup content if it exceeds the popup height.

### Help overlay

70% × 90% centered. Two-column keybind grid per section (Global / Tail / Search / Detail / Command).

```
╭─ keybindings · Esc or ? to close ───────────────────────────────────╮
│                                                                      │
│  Global                                                              │
│    Ctrl+C          quit                                              │
│    ?               toggle help                                       │
│    Esc             close overlay / return to previous focus          │
│    :               open command prompt                               │
│    Tab / Shift+Tab cycle focused pane                                │
│                                                                      │
│  Tail · Records                                                      │
│    j / ↓           cursor down                                       │
│    k / ↑           cursor up                                         │
│    g / Home        top                                               │
│    G / End         follow tail                                       │
│    ↵               open detail                                       │
│    Space           pause / resume                                    │
│    Ctrl+T          toggle absolute / relative timestamps             │
│    I W E F         toggle Info / Warn / Error / Fatal                │
│    D T             toggle Debug / Trace                              │
│    /               focus filter                                      │
│                                                                      │
│  Search                                                              │
│    t               back to tail                                      │
│    Tab / Shift+Tab cycle Query → From → To → Results                 │
│    ↵ (in Query)    submit                                            │
│    Alt+1 / 2 / 3   quick range 1h / 24h / 7d                         │
│    o               open detail for selected result                   │
│                                                                      │
│  Command mode                                                        │
│    :pause / :resume / :clear / :retry                                │
│    :save path=<file>                                                 │
│    :search q=<str> from=<iso> to=<iso>                               │
│    :level <name> on|off                                              │
│    :help / :quit                                                     │
│    Tab             complete                                          │
│    Esc             cancel                                            │
│                                                                      │
╰──────────────────────────────────────────────────────────────────────╯
```

Generated from a single `KEYBIND_TABLE` constant in `view_help.rs` so the help and the implementation cannot drift.

## Data flow (ticks and timers)

```
wall clock ─────┬──── 1 Hz tick ────► rate_history push,
                │                     flash expiry check,
                │                     redraw
                │
crossterm ─────► event ────► input::dispatch ──► AppState mutation ──► redraw
                │
websocket ─────► WsEvent ──► state.apply_ws_event ──► redraw (on frame),
                                                     rate_current += 1
```

The existing event loop in `mod.rs` already has a 100 ms redraw interval. Replace it with:
- 100 ms redraw tick (unchanged).
- 1 s rate tick — drives rate history + flash expiry.

One `tokio::select!` with three arms: `events.next()`, `ws_rx.recv()`, `rate_tick.tick()`.

## Error handling

| Scenario | Behaviour |
|---|---|
| Command prompt: unknown verb | flash_err footer, prompt stays open, input preserved |
| Command prompt: malformed kv (`:save path`) | flash_err `bad command: path=<value> required` |
| `:save` to unwritable path | flash_err with OS error message |
| `:search` with bad RFC3339 in `from`/`to` | flash_err `bad command: from=\"bogus\" is not RFC3339` |
| `syntect` fails to find JSON syntax | silently fall back to `jsonfmt::highlight` |
| Terminal too narrow (e.g. < 60 cols) | blocks squish to `Constraint::Min(…)` naturally; no crash |
| `tui-input` panic path | we wrap input handling in the existing dispatcher — no change to crash semantics |
| Ratatui 0.29 → 0.30 API surprises | caught at compile time during migration; plan covers the bump as its own task |

## Testing

### Pure-state unit tests (new, in `state.rs` / `command.rs` / `focus.rs` tests):

- `command::parse` — parsing grammar: happy paths for each verb + all error cases.
- `command::complete` — prefix completion returns expected candidates.
- `focus::cycle_forward` / `cycle_backward` — the pane cycle per mode.
- `AppState::push_rate_sample` / `rate_per_sec` — rate ring arithmetic.
- `Flash` expiry — construction + `expired_at(instant)` predicate.
- `Theme::level_color` — every level maps to the expected slot.

### Pure-view tests (in each `view_*.rs`):

- `view_tail::column_widths(area)` — visible columns at various terminal widths.
- `view_tail::render_level_cell(Level::Error, theme)` — returns expected `Span` style.
- `view_help::KEYBIND_TABLE` is non-empty + every section has at least one entry.

### Integration / snapshot (best-effort only):

- One pure `draw()` test per view that renders into a 120×40 Ratatui `TestBackend` and asserts the output buffer contains known substrings (title, column headers, footer hints). Not a full snapshot — just key landmarks.

### What we don't test:

- Full live crossterm keystroke flow (flaky in CI; manual smoke covers).
- Actual syntect output byte-exact (theme-version-dependent).
- Terminal color byte sequences (terminal-specific; we rely on Ratatui's `TestBackend`).

## Rollout

- Ships with the CLI — users pick it up via `cargo install --path crates/sift-cli --force` or the `deploy.ps1 -Cli` path landed earlier.
- No server change, no protocol change.
- Users running an old CLI against a newer server continue to work (the TUI talks to the stable `/v1/tail` + `/v1/search` endpoints).

## Open questions

None blocking. Things to revisit after this lands:

- **Saved searches / named views** — would slot in as a fourth pane on the left (yozefu-style). Post-M1.
- **Config-driven theme** — currently one dark theme; a `~/.config/sift/theme.toml` loader is straightforward to add.
- **Command history + autocomplete dropdown** — listed as non-goals but reasonable next-pass polish.

## References back

- `[[UX v2 Design]]` — spec being extended here (feature parity preserved).
- `[[CLI Overview]]` — will need an amendment noting the new keybind map + command palette.
- `PLAN.md §4.6` — original TUI scope.
- `CLAUDE.md` — dep-freshness rule this spec adheres to.
