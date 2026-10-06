#!/usr/bin/env bash
# Fetch the pinned VM helper and its matching templates and guest agent.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="2.2.1"
archive="lima-${version}-Darwin-arm64.tar.gz"
base="https://github.com/lima-vm/lima/releases/download/v${version}"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
curl -fsSL "$base/$archive" -o "$stage/$archive"
curl -fsSL "$base/SHA256SUMS" -o "$stage/SHA256SUMS"
(cd "$stage" && shasum -a 256 --check --ignore-missing SHA256SUMS)
mkdir -p "$stage/unpacked" "$root/native/build"
tar -xzf "$stage/$archive" -C "$stage/unpacked"
# only the VZ host binary and data are needed; no optional VM drivers
mkdir -p "$root/native/build/lima/bin" "$root/native/build/lima/share"
cp "$stage/unpacked/bin/limactl" "$root/native/build/lima/bin/limactl"
cp -R "$stage/unpacked/share/lima" "$root/native/build/lima/share/"
mkdir -p "$root/native/build/lima/share/doc/lima"
cp "$stage/unpacked/share/doc/lima/LICENSE" "$root/native/build/lima/share/doc/lima/"
echo "[lima] -> native/build/lima ($version)"
