# sift-cli (`sift`)

Command-line client for Sift.

## Subcommands

- `sift send [BODY]` — POST a single record to `/v1/ingest`.
- `sift search QUERY [--limit N] [--from T] [--to T]` — one-shot search.
- `sift tail` — stream `/v1/tail` to stdout.
- `sift tui` — Ratatui interactive UI (live tail + search + detail).

Global flag: `--server http://127.0.0.1:7789` (default).

## TUI keybinds

### Global
| Key | Action |
|---|---|
| `Ctrl+C` | quit |
| `?` | toggle help popup |
| `Esc` | close overlay / return to tail |
| `t` | switch to tail mode |
| `s` | switch to search mode |

### Tail mode (list focus)
| Key | Action |
|---|---|
| `q` | quit |
| `/` | focus the filter input |
| `Space` | pause / resume |
| `Up` / `Down` | move selection |
| `PageUp` / `PageDown` | scroll 10 |
| `Home` | scroll to top |
| `End` | follow tail |
| `Enter` | open detail popup |
| `I` / `W` / `E` / `F` | toggle info/warn/error/fatal visibility |
| `D` / `T` | toggle debug/trace visibility |
| `r` | force immediate reconnect attempt |

### Tail mode (filter focus)
| Key | Action |
|---|---|
| Any char | append to filter |
| `Backspace` | delete last char |
| `Esc` or `Enter` | exit filter focus |

### Search mode
| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | cycle focus (query → from → to) |
| `Enter` | submit (from query) / next field (from from/to) |
| `Up` / `Down` | move result cursor |
| `o` | open detail popup for the cursor's record |
| `Alt+1` / `Alt+2` / `Alt+3` | populate quick range 1h / 24h / 7d + submit |
| `I`/`W`/`E`/`F`/`D`/`T` | toggle level visibility |
| `Esc` | return to tail mode |

## Smoke matrix

Run after every TUI task.

### S1 — Mode switch
- Launch `sift tui` in one shell, `sift send hello` in another. ✓
- `t` (tail) / `s` (search) toggle the active view. ✓

### S2 — Tail
- `/` focuses filter; typing narrows the list. ✓
- `Space` pauses; status line shows `[paused: N new]`. ✓
- `Space` again flushes pending and resumes. ✓
- `W` toggles warn; header `[TDIWEF]` indicator dims the `W`. ✓
- Up/Down moves selection; `Enter` opens detail popup with JSON-highlighted fields. ✓

### S3 — Search
- `s`, type `error`, `Enter` → results; cursor on first row. ✓
- `Alt+1` populates From/To with "last 1h" and resubmits. ✓
- `Down`, then `o` → detail popup. Esc closes. ✓
- `Tab` cycles focus; typing appends to the focused field. ✓

### S4 — Auto-reconnect
- `nssm stop Sift` → status ticks through `reconnecting (attempt N, next in Ks)` with exponential 1/2/4/8/16/30 s delays. ✓
- `nssm start Sift` → back to `connected` after the current sleep elapses. ✓
- During backoff: press `r` → immediate retry. ✓

### S5 — Help
- `?` → popup. `Esc` closes. ✓
- With filter focused (`/`), `?` is typed as a literal — popup does NOT open. ✓
