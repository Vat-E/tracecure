# tracecure-cli (`tracecure`)

Command-line client for Tracecure.

## Subcommands

- `tracecure send [BODY] [--host H] [--app A] [--stream S] [--level L]` — POST a single record to `/v1/ingest`. BODY is read from stdin when omitted. Defaults: `host=localhost`, `app=cli`, `stream=<app>`, `level=info` (unknown level names error).
- `tracecure search QUERY [--limit N] [--from T] [--to T]` — one-shot search.
- `tracecure tail` — stream `/v1/tail` to stdout.
- `tracecure tui` — Ratatui interactive UI (live tail + search + detail).

Global flag: `--server http://127.0.0.1:7789` (default).

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

### Tail mode — `Records` focus
| Key | Action |
|---|---|
| `q` | quit |
| `s` | switch to search mode |
| `/` | focus the filter input |
| `Space` | pause / resume (no on-screen indicator — see below) |
| `↑` / `↓` / `k` / `j` | move selection |
| `PageUp` / `PageDown` | **no visible effect** — moves `state.scroll` only, which no view reads |
| `Home` / `g` | **no visible effect** — same as above; does not move the cursor |
| `End` / `G` | follow tail |
| `Enter` | open detail popup |
| `I` / `W` / `E` / `F` | toggle Info / Warn / Error / Fatal |
| `D` / `T` | toggle Debug / Trace |
| `r` | force immediate reconnect attempt |

The visible window is derived purely from `tail_cursor` (`view_tail.rs`), so only the
bindings that move the cursor — `↑`/`↓`/`k`/`j` and `End`/`G` — actually scroll the list.
`PageUp`/`PageDown`/`Home`/`g` are listed because they are bound, not because they work.

### Tail mode — `Filter` focus

Reachable via `/` only. `Tab` highlights the filter box (it moves `focus_pane`) but keys
continue to go to the record list, because routing switches on `focus`, which only `/` sets.

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
| `Esc` / `t` | return to tail mode |

Typing appends to the last focused *input* field (`Query` / `From` / `To`). Tabbing to
`Results` or `Levels` moves the border but leaves text routing on that last input field,
and the level pills are **not** operable by `h` / `l` / `Space` in search mode — use
`I W E F D T`.

### Command palette (`:`)

| Verb | Args | Effect |
|---|---|---|
| `:pause` | — | pause live tail |
| `:resume` | — | resume live tail + drain pending |
| `:clear` | — | clear the records buffer |
| `:retry` | — | force immediate reconnect |
| `:save` | `path=<file>` | serialize current buffer / results to NDJSON |
| `:search` | `q=<str>`, `from=<iso>`, `to=<iso>` | switch to search mode + submit |
| `:level` | `<name> on\|off` | toggle a level's visibility |
| `:help` | — | open help popup |
| `:quit` | — | quit |

`Tab` in the prompt completes the verb prefix; `Esc` cancels; `Enter` dispatches. Flash messages surface results (success in green, errors in red) in the footer for ~2 s.

## Smoke matrix

Run after every TUI task.

### S1 — Mode switch
- Launch `tracecure tui` in one shell, `tracecure send hello` in another. ✓
- `t` (tail) / `s` (search) toggle the active view. ✓

### S2 — Tail
- `/` focuses filter; typing narrows the list. ✓
- `Space` pauses — the list stops advancing and new records buffer silently. There is no
  on-screen paused indicator; use `:pause` for a 2 s flash confirmation. ✓
- `Space` again flushes pending and resumes — the buffered records appear at once. ✓
- `W` toggles warn; in the ` levels ` block the W pill flips from `●W` (level colour, bold)
  to `○W` (dimmed). Pill order is I W E F D T. ✓
- Up/Down moves selection; `Enter` opens detail popup with JSON-highlighted fields. ✓

### S3 — Search
- `s`, type `error`, `Enter` → results; cursor on first row. ✓
- `Alt+1` populates From/To with "last 1h" and resubmits. ✓
- `Down`, then `o` → detail popup. Esc closes. ✓
- `Tab` cycles `Query → From → To → Results → Levels`; typing appends to the last focused
  *input* field (`Results` / `Levels` do not capture text). ✓

### S4 — Auto-reconnect
Connection status is **not** rendered anywhere — `Status::label()` exists but no view calls
it, so reconnection is only observable through the flow of records.

- `nssm stop Tracecure` → records stop arriving. The client reconnects on its own with
  exponential 1/2/4/8/16/30 s delays (`ws.rs::backoff_ms`), silently. ✓
- `nssm start Tracecure` → records resume once the current sleep elapses. ✓
- During backoff: press `r` → immediate retry, so records resume without waiting out the
  remaining delay. ✓

### S5 — Help
- `?` → popup. `Esc` closes. ✓
- With filter focused (`/`), `?` is typed as a literal — popup does NOT open. ✓
