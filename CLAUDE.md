# Sift

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
