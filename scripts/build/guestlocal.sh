#!/usr/bin/env bash
# Build the guest kernel + initrd LOCALLY on Apple Silicon — no cross-compile,
# no CI. Builds a raw arm64 Image and the rootfs natively inside arm64 Linux
# containers, then stages them where a local `cargo run -p app` can
# boot the VM. Requires a running container engine (Docker/Colima/OrbStack) on
# an arm64 host. Slow the first time (kernel compile); cached after.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$root/native/build/guest"
guest="$root/native/guest"
kver="${KERNEL_VERSION:-6.12.8}"
dver="${DOCKER_VERSION:-27.5.1}"
engine="${DOCKER:-docker}"
mkdir -p "$out"

command -v "$engine" >/dev/null || { echo "need a container engine (set DOCKER=...)"; exit 1; }
"$engine" info >/dev/null 2>&1 || { echo "container engine not running — start Docker/Colima/OrbStack first"; exit 1; }

echo "[guestlocal] building arm64 kernel ${kver} (this takes a while)…"
"$engine" build --platform linux/arm64 -t hopper-kernel -f - "$guest" <<KDF
FROM arm64v8/debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential bc bison flex libssl-dev libelf-dev curl xz-utils ca-certificates kmod && \
    rm -rf /var/lib/apt/lists/*
COPY kernel.config /kernel.config
RUN curl -fsSL "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-${kver}.tar.xz" | tar -xJ && \
    cd "linux-${kver}" && make defconfig && \
    ./scripts/kconfig/merge_config.sh -m .config /kernel.config && \
    scripts/config -e IPV6 -e BRIDGE -e BRIDGE_NETFILTER -e LLC -e STP -e VLAN_8021Q && \
    make olddefconfig && \
    if ! grep -q '^CONFIG_BRIDGE=y' .config; then echo 'FATAL: CONFIG_BRIDGE not built-in'; exit 1; fi && \
    make -j"\$(nproc)" Image && cp arch/arm64/boot/Image /vmlinuz
KDF
cid="$("$engine" create --platform linux/arm64 hopper-kernel)"
"$engine" cp "$cid:/vmlinuz" "$out/vmlinuz"
"$engine" rm "$cid" >/dev/null

echo "[guestlocal] building rootfs -> initrd…"
"$engine" build --platform linux/arm64 \
  --build-arg "DOCKER_VERSION=$dver" --build-arg "DOCKER_ARCH=aarch64" \
  -f "$guest/dockerfile" -t hopper-guest "$guest"
# Pack the initramfs INSIDE the Linux container with GNU cpio — macOS bsdcpio
# produces newc archives the Linux kernel can fail to unpack (no /init → the
# guest silently never boots). -xdev keeps it on the rootfs (skips the
# runtime-mounted /proc, /sys, /dev).
"$engine" run --rm --platform linux/arm64 --entrypoint sh hopper-guest \
  -c 'cd / && find . -xdev | cpio -o -H newc 2>/dev/null | gzip -9' > "$out/initrd"

# Stage where the dev build resolves them from (see vz::provider::bundle_resources).
# executable's directory).
# The dev build reads them straight from native/build/guest.
echo "[guestlocal] done — vmlinuz + initrd staged in native/build/guest/"
file "$out/vmlinuz"