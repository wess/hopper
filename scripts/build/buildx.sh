#!/usr/bin/env bash
# Fetch the Docker Buildx CLI plugin used by Hopper's bundled Docker client.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build/buildx"
version="v0.37.2"
sha256="149e6a006edaa430f297b2bdcf65baaa6d7ee5ccfc7aa67dd50ce1c2b1db47e2"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo "error: Buildx sidecar requires an Apple silicon Mac" >&2
  exit 1
}

url="https://github.com/docker/buildx/releases/download/${version}/buildx-${version}.darwin-arm64"
curl -fsSL "$url" -o "$stage/buildx"
printf '%s  %s\n' "$sha256" "$stage/buildx" | shasum -a 256 -c -
mkdir -p "$(dirname "$out")"
cp "$stage/buildx" "$out"
chmod +x "$out"
echo "[buildx] -> native/build/buildx ($version)"
