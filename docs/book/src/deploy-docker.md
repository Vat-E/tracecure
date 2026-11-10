# Docker and Cloudflare Tunnel

The repository ships a `Dockerfile` that builds the server and the UI into one
image, and `docker/compose.yml` that runs it on a Linux host. The server serves
the UI itself (`--ui-dist`), so there is one origin and no reverse proxy.

> **There is no authentication yet.** Anyone who can reach the port can read
> and write logs. The compose file publishes it on `127.0.0.1` only. To reach
> it from elsewhere, put something that authenticates in front — the tunnel
> section below uses Cloudflare Access.

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

Programmatic ingest through the public hostname needs an Access service token.
Shippers running on the same host or LAN should post to the loopback port
instead.

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
