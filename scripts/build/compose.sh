#!/usr/bin/env bash
# Fetch the standalone Docker Compose v2 binary for the host (macOS) so Hopper
# can run Compose without a user-installed docker CLI. Compose v2 is Apache-2.0
# and runs on the host, talking to the engine via DOCKER_HOST.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build"
ver="${COMPOSE_VERSION:-v2.32.4}"

case "$(uname -m)" in
  arm64 | aarch64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) arch="$(uname -m)" ;;
esac

mkdir -p "$out"
url="https://github.com/docker/compose/releases/download/${ver}/docker-compose-darwin-${arch}"
echo "[compose] $url"
curl -fsSL "$url" -o "$out/compose"
chmod +x "$out/compose"
echo "[compose] -> $out/compose"
