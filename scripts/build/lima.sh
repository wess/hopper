#!/usr/bin/env bash
# Fetch the pinned VM helper and its matching templates and guest agent.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="2.2.1"
archive="lima-${version}-Darwin-arm64.tar.gz"
# pinned here, not read from the release: a checksum fetched beside the archive
# only proves the download was not truncated
sha256="9e9eacce88f37e185c346bad73aa6136f738d8cdf8c3bb23cd42b071824bc66e"
base="https://github.com/lima-vm/lima/releases/download/v${version}"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
curl -fsSL "$base/$archive" -o "$stage/$archive"
printf '%s  %s\n' "$sha256" "$stage/$archive" | shasum -a 256 -c -
mkdir -p "$stage/unpacked" "$root/native/build"
tar -xzf "$stage/$archive" -C "$stage/unpacked"
# only the VZ host binary and data are needed; no optional VM drivers
mkdir -p "$root/native/build/lima/bin" "$root/native/build/lima/share"
# Replace, never overwrite: writing into the inode of a helper that is running
# leaves macOS killing every later launch of it for a mismatched signature.
rm -f "$root/native/build/lima/bin/limactl"
cp "$stage/unpacked/bin/limactl" "$root/native/build/lima/bin/limactl"
cp -R "$stage/unpacked/share/lima" "$root/native/build/lima/share/"
mkdir -p "$root/native/build/lima/share/doc/lima"
cp "$stage/unpacked/share/doc/lima/LICENSE" "$root/native/build/lima/share/doc/lima/"
echo "[lima] -> native/build/lima ($version)"
