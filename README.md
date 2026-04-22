# Sift

Self-hosted Rust-native log platform. See [PLAN.md](PLAN.md) for the design and milestones.

## Status

M0 — walking skeleton.

## Quick start

```
cargo run -p sift-server -- serve --data ./data
```

Then in another terminal:

```
cargo run -p sift-cli -- send --host localhost --app demo 'hello world'
cargo run -p sift-cli -- search 'hello'
cargo run -p sift-cli -- tail
```

## Development

- `cargo build --workspace` — build server + CLI.
- `cargo test --workspace` — run all tests.
- `cd ui/sift-ui && trunk serve` — run the frontend in dev mode.
- `cd ui/sift-ui && trunk build --release` — build production UI bundle into `ui/sift-ui/dist/`.

## License

MIT OR Apache-2.0.
