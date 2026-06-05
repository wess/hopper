# hopperd

Hopper's native macOS engine helper. It boots a minimal Linux guest on Apple's
Virtualization.framework, runs `dockerd` inside it, and bridges the guest's
Docker socket out to the host so Hopper talks to it exactly as it would to
`/var/run/docker.sock`. This is what lets Hopper *be* the engine on macOS
instead of requiring Docker Desktop.

It is driven by the TypeScript `vz` provider (`src/host/engine/providers/vz`)
as a bundled sidecar, over a line-delimited JSON control protocol on stdio.

## Layout

- `config.swift` — resolves paths (kernel/initrd/disk/socket) and sizing from
  the environment Hopper sets (`HOPPER_HOME`, `HOPPER_CPUS`, `HOPPER_MEMORY_GIB`,
  `HOPPER_DISK_GIB`, `HOPPER_VSOCK_PORT`).
- `vm.swift` — builds the `VZVirtualMachineConfiguration` (Linux bootloader,
  virtio block/net/entropy, memory balloon for dynamic memory, vsock, and
  Rosetta when installed) and owns the VM lifecycle.
- `bridge.swift` — accepts connections on the host unix socket and splices each
  to a fresh vsock connection into the guest's forwarded `dockerd` port.
- `protocol.swift` — the `Command`/`Reply` wire types.
- `main.swift` — reads commands on stdin, drives the VM, replies on stdout.

## Control protocol

Commands (one JSON object per line on stdin):

```json
{"cmd":"start"}
{"cmd":"status"}
{"cmd":"stop"}
{"cmd":"ping"}
```

Replies (one per line on stdout): `{"ok":true,"state":"running","detail":"…","socket":"/Users/you/.hopper/run/docker.sock"}`.

## Build

```sh
swift build -c release            # binary at .build/release/hopperd
```

## Run (requires a guest image)

`hopperd` expects a `vmlinuz` and `initrd` next to the executable (shipped in
the bundle's `sidecars/` dir) and creates a sparse `data.img` under
`HOPPER_HOME`. Building those is the guest-image pipeline (a separate task).

## Signing / notarization

Running a VM requires the `com.apple.security.virtualization` entitlement
(`hopperd.entitlements`). Because it is a nested helper inside Hopper.app, it
must be signed **inside-out** — sign `hopperd` with its own entitlements first,
then sign the outer app — rather than with a single `codesign --deep` pass:

```sh
codesign --force --options runtime --timestamp \
  --entitlements hopperd.entitlements \
  --sign "$APPLE_SIGNING_IDENTITY" .build/release/hopperd
```
