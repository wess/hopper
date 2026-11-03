#!/usr/bin/env bash
# Fetch Docker's macOS Keychain credential helper for Hopper's CLI bundle.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build/docker-credential-osxkeychain"
version="v0.9.12"
sha256="0e43ec22d3fd186b2d69a3c76fd9ac0a1ae4ec4f2c2b2e4ee156e048c8258cb0"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || {
  echo "error: Docker credential helper requires an Apple silicon Mac" >&2
  exit 1
}

url="https://github.com/docker/docker-credential-helpers/releases/download/${version}/docker-credential-osxkeychain-${version}.darwin-arm64"
curl -fsSL "$url" -o "$stage/docker-credential-osxkeychain"
printf '%s  %s\n' "$sha256" "$stage/docker-credential-osxkeychain" | shasum -a 256 -c -
mkdir -p "$(dirname "$out")"
cp "$stage/docker-credential-osxkeychain" "$out"
chmod +x "$out"
echo "[credential-helper] -> native/build/docker-credential-osxkeychain ($version)"
