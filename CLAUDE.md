# Tracecure

Self-hosted Rust-native log platform. See [PLAN.md](PLAN.md) for the design and milestones and [docs/integration.md](docs/integration.md) for the external API.

## Companion vault — read first

This project has a durable Obsidian knowledge base at `C:\Users\vz\Programming\Vault\Sift\`. **Read its `CLAUDE.md` at the start of every task** — it defines the read/write discipline for session notes, ADRs, architecture docs, plans, and environment configs.

**At task start, cheaply read from the vault:**
- `00-Dashboard/Home.md`
- `_Index.md` in any folder you're about to use
- The most recent note in `30-Sessions/YYYY-MM/` (recovers context from the prior session)

**Write back to the vault when:**
- You finish meaningful work → session note in `30-Sessions/YYYY-MM/YYYY-MM-DD — Short Title.md`.
- You make an architectural decision → numbered ADR in `20-Decisions/`.
- You discover undocumented architecture → update `10-Architecture/<component>/`.
- You finalize an implementation plan → copy from `docs/superpowers/plans/` into the vault's `60-Tasks/Plans/`.
- Env defaults change → update `50-Environment/`.

When the vault and Claude's auto-memory (`memory/`) disagree, **the vault wins**. Auto-memory is a short-pointer cache; the vault is the source of truth.

## Working plans (in this repo)

Active TDD plans live in `docs/superpowers/plans/` during execution because the subagent-driven-development flow dispatches against them here. Once an execution is complete and the plan is the historical record, copy it into the vault at `60-Tasks/Plans/` and leave the repo copy as-is.

## Dependency freshness

**Always use the latest stable versions when adding or bumping dependencies.** Applies everywhere:

- **New dep in a design spec:** check crates.io (or npmjs.com for UI JS deps) for the latest stable as of the day the spec is written. Don't quote an older version you happen to have seen.
- **New dep in a plan:** same — the plan pins a version; it should be today's latest stable.
- **Editing `Cargo.toml`:** when a file is already being modified for an unrelated reason, opportunistically bump any other deps in that file that are more than a minor behind. Treat this as routine hygiene, not a separate task.
- **Every Cargo.toml change** gets a paired `cargo update` run before commit so the `Cargo.lock` reflects the resolver's current best pick, not a stale one.
- **Major-version bumps** are treated as intentional and called out in the commit body: mention what broke and how the migration went. Never silently pin to old majors to avoid breaking changes.
- **For the UI (wasm32) crate and its transitive web-sys/wasm-bindgen graph,** a full `cargo update` may force a WASM-toolchain upgrade. That's expected and welcome — keep up.

If a dep has been yanked, unmaintained for a year+, or has a known CVE, switch to a maintained fork or alternative crate rather than pinning to a fixed-but-stale version. Call out the switch in the commit.

Exception: workspace-pinned versions (e.g. `tokio = { version = "1.40", … }` in the root `Cargo.toml`) are deliberate and change by a single coordinated bump. Don't drift individual members off the workspace version.
