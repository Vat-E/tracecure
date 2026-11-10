# ADR-005: Sign-in for people — delegate to an OIDC provider, keep no accounts

**Status:** accepted — in force
**Date:** 2026-11-10
**Relates to:** PLAN.md §4.7 ("Auth and RBAC") and §7 (M6). Replaces §4.7's "local users + optional OIDC" with OIDC only; RBAC is not decided here. Complements [ADR-004](ADR-004-ingest-tokens.md), which covers programs that write.

## Context

Reading logs was unauthenticated. The answer until now was to keep the server on loopback or behind a proxy that authenticates, which works but leaves the server itself unable to say who is reading, and makes "who may read" a setting in someone else's product.

PLAN.md §4.7 sketched local users with argon2id passwords, JWT sessions with refresh tokens, and OIDC as an option. Since then the deployment this is being built for has gained its own identity provider. That removes the reason to build the first half: a second password database, with its own reset flow and its own breach surface, next to a provider whose whole job is that.

## Decision

**1. Tracecure has no accounts and no passwords. Sign-in is OIDC, or it is off.**
With `--oidc-issuer` set, everything that reads logs — `/v1/search`, `/v1/tail`, the UI and its assets — requires a session. Without it, the server behaves exactly as before. There is no built-in user store to fall back to.

**2. Authorization code flow with PKCE (S256), and identity from userinfo.**
The server sends the browser to the provider with a `state` and a PKCE challenge, exchanges the returned code with the verifier, then calls the provider's userinfo endpoint with the access token to learn who signed in.

It does not parse or verify an ID token. The identity is fetched by the server, from the provider, over TLS, with a token the server just obtained from that same provider — the channel authenticates the answer. That is permitted for the code flow (OpenID Connect Core §3.1.3.7, the TLS exception to signature validation) and it means no JWT library, no JWKS cache, and no key-rotation handling in a log viewer. The only cryptography here is SHA-256, for PKCE.

**3. The `state` is bound to the browser and usable once.**
It is stored server-side and also set as a short-lived cookie; the callback requires the two to match and consumes the entry. A callback link carrying someone else's `code` and `state` fails, and so does a replay.

**4. Being authenticated is not being allowed.**
A provider may let anyone register. So the operator must say who may read: `--oidc-allow-email` (the provider must report the address as verified), `--oidc-allow-group`, or — explicitly — `--oidc-allow-any-user`. Sign-in configured with none of these stops startup: it would let nobody in while looking set up.

**5. Half a configuration is an error, not "off".**
Sign-in options without an issuer, an issuer without a client id or a public URL, an unparseable value: each stops startup. The failure being designed against is a server that was meant to require sign-in starting without it because one variable was misspelled — it would serve every log to anyone and look healthy. For the same reason the provider's discovery document is fetched at startup and must name the configured issuer.

**6. Sessions are server-side, in memory, behind an opaque cookie.**
`tc_session` is 32 random bytes, `HttpOnly`, `SameSite=Lax`, `Secure` when the public URL is HTTPS, valid 30 days. Who it belongs to is held by the server, so sign-out and restart end a session for real.

In memory means a restart signs everyone out. With a provider that keeps its own session, the cost is one redirect and no prompt. Persisting sessions is deferred until that turns out to be a problem.

**7. Pages are redirected; API calls get `401`.**
A top-level page load without a session goes to `/auth/login?next=…` and comes back to the same view. `/v1/*`, asset requests and the WebSocket upgrade get `401` with a JSON body — a redirect to an HTML page is useless to `fetch`. The UI turns a `401` into the same redirect.

**8. `next` can only lead back into this server.**
It comes from the URL, so it is attacker-controlled. Anything that is not a local path — another origin, `//host`, a backslash form, the auth routes themselves — becomes `/`.

**9. With sign-in on, ingest tokens default to required.**
A server that requires sign-in is evidently reachable by people who should not get everything; leaving `/v1/ingest` open by default would be the one unguarded door. `--ingest-auth off` still works, but has to be asked for.

**10. Sign-out is a `POST`.**
So a link or an image on another page cannot end someone's session.

## What this does not do

- **No roles.** Everyone who may sign in may read everything. PLAN.md's per-stream `{read, tail, write, admin}` is still M6.
- **No single logout.** Signing out here ends this server's session, not the provider's; signing out at the provider does not end sessions here until they expire.
- **No re-check of the allow list during a session.** Removing someone from the list or the group takes effect when their session ends — up to 30 days — or at the next restart.
- **No ID token validation**, by decision 2. A deployment that cannot use TLS to the provider should not use this.
- **No API access for people.** A script cannot read logs with a personal token; reading is browser sessions only.

## Consequences

- The server can be exposed directly: reading behind sign-in, writing behind tokens, nothing depending on a proxy for authentication. A proxy is still needed for TLS.
- The provider is a hard dependency at startup and at each sign-in. If it is down, existing sessions keep working and new sign-ins fail.
- `reqwest` becomes a runtime dependency of the server (it was a test dependency).
- The UI's assets are behind the session too, so an unauthenticated visitor receives nothing of the application.
