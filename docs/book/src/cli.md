# CLI and TUI

Both ship in one binary, `tracecure`. The TUI is a subcommand, not a separate
artifact, so it reuses the same global flags and the same wire types as the
one-shot commands.

The authoritative, always-current reference — including the full keybinding
table and the command-palette verbs — is
[`crates/tracecure-cli/README.md`](https://github.com/tracecure-dev/tracecure/blob/main/crates/tracecure-cli/README.md)
in the repo. This chapter is the orientation.

## Global flag

`--server <url>` applies to every subcommand. Default `http://127.0.0.1:7789`.

## One-shot commands

```
tracecure send --host <host> --app <app> "message"
tracecure search <substring>
tracecure tail
```

`send` posts a single record. `search` does a substring match over `body` and
prints the matches. `tail` streams the live WebSocket line by line and exits
cleanly when the server closes the socket — it composes with `grep`, `head` and
friends.

## Interactive TUI

```
tracecure tui
```

Ratatui + crossterm. This is the SSH-into-a-box case, where a browser is not
available.

- `/` — client-side substring filter over the buffered records
- `:` — command palette (pause, resume, clear, level toggles, save, help)
- `?` — keybinding overlay
- `Enter` — expand a record's labels, fields and raw payload
- `Space` — pause/resume the live tail
- Arrow keys / `PageUp` / `PageDown` / `Home` / `End` — move the cursor; the
  viewport follows it
- `q` or `Ctrl+C` — quit

The palette accepts a leading `:` optionally, level names case-insensitively,
and quoted values. `:save <path>` writes the buffered records as NDJSON.

Terminal state is restored on panic — the alt screen and raw mode are unwound
by a guard, so a crash does not leave your shell wedged.

## Why a subcommand rather than a second binary

One artifact to ship, one set of global flags, and the same `tracecure-api`
types as every other client. The web UI, the TUI and the CLI all render the
same `TailFrame` stream, so a wire-shape change is a compile error in all three
rather than a runtime surprise in one.
