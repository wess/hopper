#!/usr/bin/env bash
# Fetch the standalone Docker Compose v2 binary for the host (macOS) so Hopper
# can run Compose without a user-installed docker CLI. Compose v2 is Apache-2.0
# and runs on the host, talking to the engine via DOCKER_HOST.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build"
ver="v5.7.0"
sha256="54495ff81dd8cae11861455f0308a293a530149beba8f1d90ff3f295b11cf035"

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo "error: Compose sidecar requires an Apple silicon Mac" >&2
  exit 1
}

mkdir -p "$out"
url="https://github.com/docker/compose/releases/download/${ver}/docker-compose-darwin-aarch64"
echo "[compose] $url"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
curl -fsSL "$url" -o "$stage/compose"
printf '%s  %s\n' "$sha256" "$stage/compose" | shasum -a 256 -c -
cp "$stage/compose" "$out/compose"
chmod +x "$out/compose"
echo "[compose] -> $out/compose"
