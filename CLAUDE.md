# Tracecure

Self-hosted, Rust-native log platform. Single binary: ingest, store, query,
serve the UI.

- [PLAN.md](PLAN.md) — architecture and the M0–M6 milestone ladder.
- [docs/book/](docs/book/) — user-facing documentation (mdBook).
- [docs/decisions/](docs/decisions/) — ADRs. **Read the relevant one before
  changing a decision it records.**

Project knowledge lives in this repo. There is no external vault; anything
worth keeping goes in an ADR, a doc, or a `CLAUDE.md`.

## Where the instructions live

Per-area guidance sits in nested `CLAUDE.md` files and loads only when you work
in that subtree — keep area-specific detail there, not here:

| Path | Covers |
|---|---|
| `crates/tracecure-api/CLAUDE.md` | the wire contract; serde attributes that ARE the contract |
| `crates/tracecure-storage/CLAUDE.md` | ADR-governed writer knobs, crash-safety invariants |
| `ui/tracecure-ui/CLAUDE.md` | Leptos signal granularity, the `<For>` key rule, browser verification |

## Layout gotchas

`ui/tracecure-ui` and `bench/format-fit` are **excluded from the cargo
workspace** (root `Cargo.toml`). So:

- `cargo test --workspace` does **not** cover the UI or the bench. Test the UI
  from inside `ui/tracecure-ui`.
- The UI's real check is `cargo clippy --target wasm32-unknown-unknown --all-targets`.

## Before you commit

`Cargo.toml` enables `clippy::pedantic` workspace-wide and CI sets
`RUSTFLAGS: -D warnings`, so pedantic lints are hard errors:

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI (`.github/workflows/ci.yml`) also enforces a 600 KB gzipped-wasm budget.
**The repo has no git remote, so CI never actually runs** — these local
commands are the only gate.

## Dependency freshness

Use the latest stable version when adding or bumping a dependency; check
crates.io (or npmjs.com) rather than quoting a version from memory. Pair every
`Cargo.toml` change with `cargo update` in the same commit so `Cargo.lock`
reflects the resolver's current pick.

Major-version bumps are intentional: call out in the commit body what broke and
how the migration went. Never pin to an old major to dodge a break. If a dep is
yanked, unmaintained for a year+, or has a known CVE, switch to a maintained
alternative and say so in the commit.

Exception: workspace-pinned versions (e.g. `tokio` in the root `Cargo.toml`)
change by one coordinated bump — don't drift individual members off them.

A full `cargo update` in `ui/tracecure-ui` may force a WASM-toolchain upgrade.
That is expected; keep up.

## Verifying a change in the real app

`.\deploy.ps1 -Ui` rebuilds just the UI (no admin, no service restart) and is
the cheap inner loop. `.\deploy.ps1` alone is the full elevated deploy.

Do not trust `trunk serve` for anything involving live tail: its WebSocket proxy
accepts the `/v1/tail` upgrade and then forwards no frames. Serve the built
assets from the server instead — see `ui/tracecure-ui/CLAUDE.md`.

## Writing docs

Correct the doc in the same commit as the code. The docs that must track code
are `README.md`, `PLAN.md`, `docs/book/`, and the per-crate `README.md`s.

ADRs and `docs/m1-format-fit-report.md` are **historical records** — they
describe a past state on purpose. Do not "update" them to match current code;
supersede them with a new ADR and mark the old one's status.
