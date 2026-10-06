#!/usr/bin/env bash
# The standalone host CLI for Docker contexts and Compose compatibility.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="27.5.1"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
curl -fsSL "https://download.docker.com/mac/static/stable/aarch64/docker-${version}.tgz" -o "$stage/docker.tgz"
tar -xzf "$stage/docker.tgz" -C "$stage"
mkdir -p "$root/native/build"
# old guest builds used a directory here; an empty leftover can be replaced
if [ -d "$root/native/build/docker" ]; then rmdir "$root/native/build/docker"; fi
cp "$stage/docker/docker" "$root/native/build/docker"
echo "[docker] -> native/build/docker ($version)"
