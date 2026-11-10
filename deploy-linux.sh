#!/usr/bin/env bash
# Build Tracecure on a remote Linux host and (re)start it there.
#
#   ./deploy-linux.sh <ssh-host> [remote-dir]
#
# <ssh-host>    anything `ssh` accepts: a Host alias, user@host, ...
# [remote-dir]  where compose.yml and ./data live on the host
#               (default: ~/tracecure)
#
# The host needs Docker with the compose plugin and nothing else: the source
# is streamed over SSH as the build context and the image is built there.
# See docs/book/src/deploy-docker.md.
set -euo pipefail

if [ $# -lt 1 ]; then
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
  exit 2
fi

host=$1
remote_dir=${2:-tracecure}

cd "$(dirname "$0")"

tag=$(git rev-parse --short HEAD)
# The working tree is what gets sent, so say so when it differs from HEAD.
if [ -n "$(git status --porcelain)" ]; then
  tag="$tag-dirty"
fi

echo "==> Building tracecure:$tag on $host"
# Tracked files plus untracked-but-not-ignored ones: exactly what a commit of
# the current tree would contain. .dockerignore trims it further on the host.
git ls-files -co --exclude-standard -z \
  | tar --null -T - -czf - \
  | ssh "$host" "docker build -q -t tracecure:$tag -t tracecure:latest - >/dev/null"

echo "==> Installing compose file in $host:$remote_dir"
ssh "$host" "mkdir -p '$remote_dir/data'"
scp -q docker/compose.yml docker/vector.yaml "$host:$remote_dir/"

echo "==> Restarting"
# Only the `tracecure` service: a tunnel connector or a log shipper started
# with `--profile tunnel` / `--profile logs` keeps running and reconnects to
# the new container. Their own config changes are picked up by re-running
# `docker compose --profile <name> up -d` on the host.
ssh "$host" "cd '$remote_dir' && docker compose up -d tracecure"

echo "==> Waiting for /health"
ssh "$host" '
  port=${TRACECURE_PORT:-7789}
  for i in $(seq 1 30); do
    if [ "$(curl -s -m 2 "http://127.0.0.1:$port/health")" = "ok" ]; then
      echo "healthy"
      exit 0
    fi
    sleep 1
  done
  echo "not healthy after 30s; last container log lines:" >&2
  docker logs --tail 20 tracecure >&2
  exit 1
'

echo "==> Deployed tracecure:$tag to $host"
