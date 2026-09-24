#!/usr/bin/env bash
# Fetch the standalone Docker Compose v2 binary for the host (macOS) so Hopper
# can run Compose without a user-installed docker CLI. Compose v2 is Apache-2.0
# and runs on the host, talking to the engine via DOCKER_HOST.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build"
ver="${COMPOSE_VERSION:-v2.32.4}"

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo "error: Compose sidecar requires an Apple silicon Mac" >&2
  exit 1
}

mkdir -p "$out"
url="https://github.com/docker/compose/releases/download/${ver}/docker-compose-darwin-aarch64"
echo "[compose] $url"
curl -fsSL "$url" -o "$out/compose"
chmod +x "$out/compose"
echo "[compose] -> $out/compose"
