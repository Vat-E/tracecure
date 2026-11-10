# ADR-004: Ingest tokens ahead of M6 — write-only bearer tokens in a file

**Status:** accepted — in force
**Date:** 2026-11-10
**Relates to:** PLAN.md §4.7 ("Auth and RBAC") and §7 (M6). Pulls one piece of M6 forward; the rest of M6's auth is not decided here.

## Context

PLAN.md puts all authentication in M6, and until then the answer was "bind to loopback or a trusted network". That holds while every writer runs next to the server. It stops holding the moment a service somewhere else needs to send logs: the only way in was to put the whole server behind a reverse proxy that authenticates, and give each writer a credential *for the proxy* — one shared secret per deployment, managed in someone else's console, with no way to tell writers apart or cut one off.

What is needed now is narrow: let a program write, know which program it was, and be able to revoke it. Reading logs and the UI are a different problem (people, sessions, an identity provider) and stay behind whatever fronts the server.

## Decision

**1. A token grants one thing: writing to `/v1/ingest`.**
There is no read scope and nothing else is guarded by a token. `scope: "ingest"` is stored on each token anyway, so that when a read scope exists, old tokens are unambiguous.

**2. Format: `tc_<id>_<secret>`.**
`id` is 8 hex characters and is public — it is what `token list` shows and `token revoke` takes. `secret` is 32 random bytes as 64 hex characters, from the OS random source. The prefix makes a leaked token recognisable to secret scanners and to a human reading a log.

**3. Only the SHA-256 of the secret is stored.**
In `{data}/tokens.json`, mode 0600. A fast hash is correct here, unlike for passwords: the secret is 256 bits of randomness, so there is nothing to brute-force that a slow KDF would protect. Verification looks the token up by `id` and compares hashes in constant time.

**4. The token is shown once.**
`tracecure-server token create --name <name>` prints it on stdout (and nothing else there, so it can be piped) and never again. A lost token is replaced, not recovered.

**5. Enforcement is an explicit mode, off by default.**
`--ingest-auth required` (or `TRACECURE_INGEST_AUTH=required`). Not "on as soon as a token exists": that would make creating the first token silently lock out every existing writer. An unknown value for the mode stops startup rather than being read as `off`.

**6. The server follows the file.**
It re-reads `tokens.json` when its mtime or length changes, so `token create` and `token revoke` — run as a separate process, e.g. `docker compose exec` — take effect on the running server without a restart. Writes go through a temp file and a rename, so the server never reads a half-written list. If the file is present but unreadable, every token is rejected.

**7. The check comes before the body.**
An unauthenticated request is answered `401` with `WWW-Authenticate: Bearer` before the body is parsed. The response says what was wrong with the request and nothing about which tokens exist.

## What this does not do

- **No transport security.** A bearer token over plain HTTP is readable by anyone on the path. Tokens assume TLS in front — a tunnel or a reverse proxy.
- **No rate limiting or quota.** A valid token can write as fast as the server accepts. The damage a leaked token can do is bounded by revoking it and by retention (ADR-003), not by the token.
- **No expiry.** Tokens live until revoked.
- **No per-token restriction** on which `app` or `stream_id` it may write. A token for project A can write records labelled as project B.
- **No audit trail** of which token wrote what. The server logs the token id at debug level per request; it is not stored with the records.
- **Not user authentication.** People reading logs are out of scope; see below.

## Relationship to M6

M6 still owns authentication for people: the UI and the read endpoints, most likely by delegating to an external identity provider rather than keeping accounts here. When that lands, these tokens are the machine half of it. The pieces M6 will want to add to them are the last four gaps above — scoping a token to a stream, expiry, quotas, and attribution.

## Consequences

- `/v1/ingest` can be exposed to the internet on its own, with everything else kept private. The deployment has to do that deliberately (a path rule in the proxy); nothing here opens anything.
- Every writer needs a token once the mode is on, including ones on the same host. The Docker log shipper reads its token from a mounted file.
- `tokens.json` is now part of the data directory and of any backup of it. It holds no secrets, but restoring an old copy un-revokes tokens revoked since.
- Two new dependencies in the server: `sha2` and `getrandom`.
