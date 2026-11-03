#!/usr/bin/env bash
# The standalone host CLI for Docker contexts and Compose compatibility.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="29.9.2"
sha256="ea8a2ce5ac31f9f5f3fb711a7a485e1e46cc7a69798a629a70306f6e6af25b7a"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
curl -fsSL "https://download.docker.com/mac/static/stable/aarch64/docker-${version}.tgz" -o "$stage/docker.tgz"
printf '%s  %s\n' "$sha256" "$stage/docker.tgz" | shasum -a 256 -c -
tar -xzf "$stage/docker.tgz" -C "$stage"
mkdir -p "$root/native/build"
# old guest builds used a directory here; an empty leftover can be replaced
if [ -d "$root/native/build/docker" ]; then rmdir "$root/native/build/docker"; fi
cp "$stage/docker/docker" "$root/native/build/docker"
echo "[docker] -> native/build/docker ($version)"
