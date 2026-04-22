# Sift

Self-hosted Rust-native log platform. See [PLAN.md](PLAN.md) for the design and milestones.

## Status

M0 — walking skeleton. See [docs/integration.md](docs/integration.md) for how to send records from other apps.

## Quick start

```
cargo run -p sift-server -- serve
```

The server writes to `./data/sift.ndjson` by default and binds to `127.0.0.1:7789`.
Override with `--data <path>` and `--bind <addr>`.

Then in another terminal:

```
cargo run -p sift-cli -- send --host localhost --app demo 'hello world'
cargo run -p sift-cli -- search hello
cargo run -p sift-cli -- tail
```

To serve the UI from the same binary, first build it:

```
(cd ui/sift-ui && trunk build --release)
cargo run -p sift-server -- serve --ui-dist ./ui/sift-ui/dist
```

Then open http://127.0.0.1:7789/ in a browser.

## Development

- `cargo build --workspace` — build server + CLI.
- `cargo test --workspace` — run all tests.
- `cd ui/sift-ui && trunk serve` — run the frontend in dev mode.
- `cd ui/sift-ui && trunk build --release` — build production UI bundle into `ui/sift-ui/dist/`.

## License

MIT OR Apache-2.0.
