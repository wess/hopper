# Hopper guest image

The Linux guest that Hopper boots on Apple's Virtualization.framework to run
the Docker engine. The OS lives in an initramfs (read-only, shipped in the app
bundle); the host supplies a persistent data disk for `/var/lib/docker`.

## Pieces

- `dockerfile` — the rootfs: Alpine + the Apache-2.0 static Docker Engine
  (dockerd, containerd, runc, docker-init, docker-proxy) + `socat` + the tools
  `init` needs. All permissively licensed; no Docker Desktop bits, no Docker
  trademarks shipped.
- `init` — PID 1 in the initramfs. Mounts the pseudo-filesystems, formats and
  mounts the data disk (`/dev/vda` → `/var/lib/docker`) on first boot, starts
  `dockerd`, then forwards its unix socket onto **vsock port 2375**. The
  host-side bridge in `engine::vz::bridge` dials that port and re-exposes it as a unix
  socket Hopper talks to.

## Build

```sh
HOPPER_KERNEL=/path/to/Image \
  scripts/build/guest.sh           # rootfs -> initrd, plus the kernel
```

Outputs land in `native/build/guest/{vmlinuz,initrd}`,
which `butter.yaml` lists as sidecars — they're copied into the bundle's
`Contents/MacOS/sidecars/` and resolved at runtime by the `vz` provider.

## The kernel

Virtualization.framework's `VZLinuxBootLoader` needs a **raw, uncompressed
arm64 Linux `Image`** — not a gzipped `vmlinuz`. Options:

- Extract and decompress one from a distro kernel package (e.g. Alpine's
  `linux-virt`), or
- Build a minimal kernel with the virtio drivers the guest needs
  (`VIRTIO_BLK`, `VIRTIO_NET`, `VIRTIO_CONSOLE`, `VSOCKETS`,
  `VIRTIO_VSOCKETS`, `EXT4_FS`, `CGROUPS`, `OVERLAY_FS`, `BRIDGE`,
  `NETFILTER`), then point `HOPPER_KERNEL` at the resulting `arch/arm64/boot/Image`.

This raw-kernel requirement is the one maintainer-supplied input; everything
else is reproducible from `dockerfile` + `init`.
