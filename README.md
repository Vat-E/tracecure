# Tracecure

Self-hosted Rust-native log platform. See [PLAN.md](PLAN.md) for the design and milestones.

## Status

M0 — walking skeleton. See [docs/integration.md](docs/integration.md) for how to send records from other apps.

## Quick start

```
cargo run -p tracecure-server -- serve
```

The server stores data under `./data/` by default and binds to `127.0.0.1:7789`.
Override with `--data <path>` and `--bind <addr>`.

Then in another terminal:

```
cargo run -p tracecure-cli -- send --host localhost --app demo 'hello world'
cargo run -p tracecure-cli -- search hello
cargo run -p tracecure-cli -- tail
cargo run -p tracecure-cli -- tui      # interactive terminal UI with filter + scroll
```

To serve the UI from the same binary, first build it:

```
(cd ui/tracecure-ui && trunk build --release)
cargo run -p tracecure-server -- serve --ui-dist ./ui/tracecure-ui/dist
```

Then open http://127.0.0.1:7789/ in a browser.

## Development

- `cargo build --workspace` — build server + CLI.
- `cargo test --workspace` — run all tests.
- `cd ui/tracecure-ui && trunk serve` — run the frontend in dev mode.
- `cd ui/tracecure-ui && trunk build --release` — build production UI bundle into `ui/tracecure-ui/dist/`.

## License

MIT OR Apache-2.0.
