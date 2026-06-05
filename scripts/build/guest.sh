#!/usr/bin/env bash
# Produce the Hopper guest assets (vmlinuz + initrd) that hopperd boots.
#
# Builds the rootfs from native/guest/dockerfile with whatever container engine
# is on PATH, exports it, and packs it into a gzipped cpio initramfs. The
# kernel must be a RAW arm64 Linux Image (Virtualization.framework does not
# accept a gzipped vmlinuz) — supply it via HOPPER_KERNEL. See
# native/guest/readme.md.
#
# Output: native/hopperd/.build/guest/{vmlinuz,initrd} — picked up by the
# sidecar list in butter.yaml at bundle time.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
guest="$root/native/guest"
out="$root/native/hopperd/.build/guest"
arch="${ARCH:-aarch64}"
engine="${DOCKER:-docker}"
mkdir -p "$out"

if ! command -v "$engine" >/dev/null 2>&1; then
  echo "error: need a container engine to build the rootfs (set DOCKER=...)." >&2
  exit 1
fi

echo "[guest] building rootfs ($arch)"
"$engine" build \
  --platform "linux/${arch/aarch64/arm64}" \
  --build-arg "DOCKER_ARCH=$arch" \
  -f "$guest/dockerfile" \
  -t hopper-guest "$guest"

echo "[guest] exporting rootfs -> initramfs"
cid="$("$engine" create hopper-guest)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"; "$engine" rm -f "$cid" >/dev/null 2>&1 || true' EXIT
"$engine" export "$cid" | tar -x -C "$tmp"
( cd "$tmp" && find . | cpio -o -H newc 2>/dev/null | gzip -9 ) > "$out/initrd"

if [ -n "${HOPPER_KERNEL:-}" ]; then
  cp "$HOPPER_KERNEL" "$out/vmlinuz"
  echo "[guest] kernel <- $HOPPER_KERNEL"
else
  echo "[guest] WARNING: no HOPPER_KERNEL set — initrd built, but vmlinuz is missing." >&2
  echo "[guest] Supply a raw arm64 Linux Image: see native/guest/readme.md." >&2
fi

echo "[guest] assets -> $out"
