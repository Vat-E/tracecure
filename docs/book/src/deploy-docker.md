# Docker and Cloudflare Tunnel

The repository ships a `Dockerfile` that builds the server and the UI into one
image, and `docker/compose.yml` that runs it on a Linux host. The server serves
the UI itself (`--ui-dist`), so there is one origin and no reverse proxy.

> **Open by default.** Until you configure it otherwise, anyone who can reach
> the port can read every log and write new ones. The compose file publishes
> it on `127.0.0.1` only. Before exposing it, either put something that
> authenticates in front (the tunnel section below uses Cloudflare Access) or
> turn on Tracecure's own [sign-in](#sign-in-without-a-proxy) and
> [ingest tokens](#let-services-elsewhere-send-logs).

## Build

```
docker build -t tracecure .
```

The first build compiles `trunk` and takes a while; later builds reuse that
layer. The image runs as an unprivileged user and keeps its data in `/data`.

## Deploy to a remote host in one step

`deploy-linux.sh` does the build and the restart on a host you can reach over
SSH. Nothing but Docker is needed there — no Rust toolchain, no checkout:

```
./deploy-linux.sh my-host            # deploy the working tree
./deploy-linux.sh my-host ~/tracecure
```

It streams the tracked files of the working tree to the host, builds the image
there, tags it with the commit, copies `docker/compose.yml` next to the data
directory and runs `docker compose up -d`. A running tunnel connector is left
alone; data in `./data` is untouched. It then waits for `/health`.

Because it sends the working tree, uncommitted changes are deployed too — the
tag gets a `-dirty` suffix so the running image says so.

## Run

Copy `docker/compose.yml` to a directory on the host, then:

```
mkdir data
docker compose up -d
curl http://127.0.0.1:7789/health
```

| Variable | Default | Meaning |
|---|---|---|
| `TRACECURE_IMAGE` | `tracecure:latest` | image to run |
| `TRACECURE_PORT` | `7789` | loopback port on the host |
| `TRACECURE_UID` / `TRACECURE_GID` | `1000` | owner of `./data` on the host |
| `TRACECURE_RETENTION` | empty | delete records older than this, e.g. `30d`; empty keeps everything |
| `TRACECURE_RETENTION_STREAMS` | empty | per-stream ages, `stream_id=duration`, comma-separated |
| `TRACECURE_INGEST_AUTH` | empty (`off`) | `required` makes `/v1/ingest` demand a token |
| `TRACECURE_OIDC_ISSUER`, `TRACECURE_OIDC_CLIENT_ID`, `TRACECURE_PUBLIC_URL`, `TRACECURE_OIDC_ALLOW_EMAILS`, … | empty | sign-in through an OIDC provider — see [Sign-in without a proxy](#sign-in-without-a-proxy) |

Put them in a `.env` file next to `compose.yml`. Retention is described in
[Install and run](./install.md#retention); without it the data directory only
grows.

WAL and segment files live in `./data` next to the compose file. Back that
directory up; removing the container does not touch it.

## Publish through a Cloudflare Tunnel

The UI builds its API and WebSocket URLs from the page origin, so the tunnel
must publish the whole server under one hostname — do not split the UI onto a
separate static host.

1. In the Cloudflare Zero Trust dashboard create a tunnel and add a public
   hostname whose service is `http://127.0.0.1:7789`.
2. **Before** the hostname is reachable, add an Access application for it with
   a policy that allows only you. Without this the instance is open to the
   internet.
3. Save the tunnel token next to the compose file:

   ```
   install -m 600 /dev/null tunnel-token
   $EDITOR tunnel-token
   ```

4. Start the connector:

   ```
   docker compose --profile tunnel up -d
   ```

Live tail uses a WebSocket on `/v1/tail`; Cloudflare Tunnel forwards it without
extra configuration.

Behind Access, a program cannot log in, so nothing outside the host can send
logs through the public hostname as it stands. The next section opens that one
path.

## Let services elsewhere send logs

The aim: `/v1/ingest` reachable from the internet and guarded by Tracecure's
own tokens, everything else still behind Access.

1. Turn the tokens on. In `.env` next to `compose.yml`:

   ```
   TRACECURE_INGEST_AUTH=required
   ```

   **Do this first.** Until it is on, the next step would make ingest open to
   the world.

2. Give the log shipper a token, if you run it — it is a writer too:

   ```
   docker compose up -d tracecure
   token=$(docker compose exec -T tracecure tracecure-server token create --name shipper --data /data)
   (umask 077 && printf '{"token":"%s"}\n' "$token" > shipper-token.json)
   docker compose --profile logs up -d --force-recreate shipper
   ```

3. Check that ingest now refuses a request without a token:

   ```
   curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:7789/v1/ingest   # 401
   ```

4. Only then, in Cloudflare Access, add a second application for the single
   path `<hostname>/v1/ingest` with a **Bypass** policy for everyone. Access
   matches the most specific path, so this one path skips the login and the
   rest of the hostname keeps it.

5. Create a token per service that will send logs, and hand each its own:

   ```
   docker compose exec -T tracecure tracecure-server token create --name billing-service --data /data
   ```

From outside, an unauthenticated `POST` to `/v1/ingest` should now return
`401` from Tracecure, and any other path should still redirect to the Access
login. What a token can and cannot do is in
[the integration chapter](./integration.md#ingest-tokens).

## Sign-in without a proxy

Tracecure can require sign-in itself, through an OpenID Connect provider, so
the tunnel no longer needs an Access application in front of it. The flags and
what they mean are in [Install and run](./install.md#sign-in); in Docker they
go in `.env`:

```
TRACECURE_OIDC_ISSUER=https://login.example.com
TRACECURE_OIDC_CLIENT_ID=tracecure
TRACECURE_PUBLIC_URL=https://logs.example.com
TRACECURE_OIDC_ALLOW_EMAILS=you@example.com
```

Order matters when switching over from Access, because for a moment both are
in play:

1. Register the client at the provider with the redirect URI
   `https://logs.example.com/auth/callback` and the post-logout redirect URI
   `https://logs.example.com/auth/signed-out`.
2. Give the log shipper a token if you run it (step 2 of
   [Let services elsewhere send logs](#let-services-elsewhere-send-logs)) —
   sign-in makes ingest tokens required.
3. Set the variables and `docker compose up -d tracecure`. Check the log for
   `sign-in required to read logs`.
4. Sign in through the still-protected hostname and confirm it works end to
   end: Access first, then the provider, then the UI.
5. Only then remove the Access application. From outside, the hostname should
   now redirect to the provider, `/v1/search` should answer `401`, and
   `/v1/ingest` should answer `401` without a token.

If step 3 fails the server does not start, and says why. It never starts with
reading open because of a configuration mistake.

## Collect the logs of the other containers on the host

The compose file has an optional `shipper` service (profile `logs`): a
[Vector](https://vector.dev) container that reads the stdout/stderr of every
other container on the host through the Docker socket and posts it to
Tracecure. Nothing in those containers changes.

Copy `docker/vector.yaml` next to `compose.yml`, then:

```
docker compose --profile logs up -d
```

| Variable | Default | Meaning |
|---|---|---|
| `SHIPPER_HOST` | `docker-host` | the `host` field on every shipped record |
| `SHIPPER_EXCLUDE` | empty | container-name prefixes to leave out, comma-separated |

Put them in a `.env` file next to `compose.yml`.

The shipper also mounts `shipper-token.json` — `{"token": "tc_…"}` — and sends
it with every batch. The file has to exist: `deploy-linux.sh` creates it with
an empty token, which is all a server with ingest auth off needs. With auth
on, put a real token in it (see
[Let services elsewhere send logs](#let-services-elsewhere-send-logs)).

What a shipped record looks like:

- `app` is the container name, `stream_id` is `docker/<container name>`.
- `body` is the log line, with ANSI colour codes removed. Blank lines are
  dropped.
- `labels` carry `source: docker`, the `image`, and `stream` (`stdout` or
  `stderr`).
- `level` is **a guess**. Container output is text with no level field, so the
  shipper looks for a severity word in the line and falls back to `info`. A
  line that merely mentions "error" is tagged as one. A service that logs
  structured records should post to `/v1/ingest` itself — see the
  [integration chapter](./integration.md).

Things to decide before turning it on:

- **It only ships what is logged after it starts.** There is no backfill of a
  container's existing log.
- **Exclude what should not be in a log viewer.** A password manager's log
  holds e-mail addresses and login IPs; a database's can hold query text.
  Tracecure's own containers are always excluded — shipping the server's log
  of an ingest back into it is a loop.
- **The Docker socket mount is root-equivalent on the host**, read-only flag
  notwithstanding. That is true of every log agent that works this way; run
  it only where you would run one.
- **Nothing is ever deleted yet.** Retention by time is not implemented, so
  the data directory only grows.
- A multi-line stack trace arrives as one record per line.
