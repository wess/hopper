# Hopper

A native desktop app for running containers and desktop virtual machines — a Docker Desktop
replacement written in Rust. The UI is [gpui](https://github.com/zed-industries/zed)
+ [guise](https://github.com/wess/guise); the async layer is Tokio.

Hopper currently targets **Apple silicon Macs running macOS 26 or later**. Its
engine is a persistent Linux VM running Docker, managed through a bundled
[Lima](https://lima-vm.io/) Virtualization.framework helper. Hopper creates,
starts, and stops the VM for you. It can also attach to a Docker-compatible endpoint
on the Mac, and **Import from Docker** brings your images and containers across.

No bundled browser or Electron. The VM runs in a separate helper process, so a
UI crash does not take down containers. Settings controls whether a normal app
quit stops the engine. Releases bundle the VM helpers, Docker CLI, Compose, and the Windows QEMU/TPM runtime.

## Features

- **Virtual machines** — Ubuntu Desktop, macOS, and Windows 11 ARM64 profiles,
  dedicated viewer windows, disk snapshots, recovery, and independent clones.
  OS images download on first start. Windows fetches Microsoft installation media
  and prepares its bootable ARM64 installer automatically; a local English US ISO
  is optional. macOS and Windows guest support is experimental. Windows has no accelerated 3D graphics.
  Guest VMs live under `~/.hopper/machines`, share no host folders, and remain
  running when Hopper quits.
- **Agent access** — enabled for new VMs, revocable per VM. The MCP server
  exposes guest commands, screenshots, files, input, cloning, and snapshots.
  Linux supports text, keyboard, and mouse input. Windows supports QEMU keys
  and mouse input; macOS input requires Accessibility permission inside the guest.
  Snapshots and clones require a stopped VM. Clones inherit agent access.

- **Dashboard** — running/total containers, image/volume/network counts, disk
  usage with reclaimable meters, and one-click "Clean up" (system prune).
- **Containers** — live list with search, "only running" filter, and per-row
  lifecycle (start/stop/restart). A detail pane opens beside the list with tabs:
  - **Logs** — live streaming, demuxed stdout/stderr, stderr coloured
  - **Stats** — live CPU / memory / network / block-IO / PID meters
  - **Files** — browse the container's filesystem and copy files out
  - **Terminal** — a real interactive shell (socket hijack + a minimal terminal
    emulator, not a line-buffered fake)
  - **Inspect** — the full JSON tree
- **Images** — list with in-use state, pull with live layer progress, build from
  a Dockerfile (`.dockerignore` honoured, including negations), push, tag,
  history, remove, prune, save/load.
- **Volumes** — list with in-use detection, create, remove, prune.
- **Networks** — list, create, connect/disconnect, remove, prune (built-ins
  protected).
- **Stacks** — compose projects reconstructed from container labels, so they
  appear with no compose CLI and start/stop label-driven.
- **Settings** — choose Automatic, Hopper Engine, Apple Containers, Docker, Podman, Colima,
  Rancher Desktop, or an existing endpoint; control lifecycle, appearance,
  VM CPU/memory/disk resources, and Docker CLI integration.
- **Migration** — scan a source engine (Docker Desktop / Colima / Rancher) and
  copy images, networks, and container configuration into Hopper's engine;
  named volumes referenced by recreated containers are created as needed,
  while selected volume contents are called out for manual transfer rather
  than being silently lost.
- **MCP server** — a standalone stdio Model Context Protocol server
  (`hoppermcp`) exposing Docker and VM tools to clients. Requests run concurrently; cancellation
  stops the request without shutting down the VM.

### Engines

- **macOS — Hopper Engine.** A single headless Linux VM runs a rootful Docker
  daemon. First startup downloads Linux and provisions Docker; subsequent starts
  reuse the disk. VM data lives under `~/.hopper/engine/lima`, separate from any
  existing Lima or Colima installation (`HOPPER_DIR` overrides the root).
  The user's home folder is shared through virtiofs. Published TCP ports are
  forwarded to localhost. CPU and memory apply after stopping and starting;
  disk size applies at creation. Retry restarts an unresponsive VM and keeps its
  disk. Startup diagnostics are in `~/.hopper/engine/lima/engine.log`.
- **Apple Containers (optional).** Needs an Apple silicon Mac running macOS 26. If it is not installed, Hopper
  offers to fetch Apple's signed installer and hands it to the system installer;
  Hopper never elevates. After that it starts and stops the services itself.
- **Existing engines.** On macOS, Hopper can attach to Docker Desktop, Podman,
  Colima, Rancher Desktop, or a remote Docker-compatible endpoint.

Apple's runtime is not the Engine API, and Hopper does not pretend otherwise:
pause, rename, post-create resource changes, restart policies, healthchecks,
and the event stream do not exist there, so the UI hides
them or explains the limitation rather than offering a button that fails.
Anything that needs them can switch engines in Settings.

### Import from Docker

Copies images, networks and containers out of Docker Desktop, Colima or Rancher
Desktop and into whichever engine Hopper is running. Containers are recreated
rather than moved — the image, ports, mounts and labels travel; a writable layer
is by definition scratch. Nothing is removed from the source.

## Architecture

A Cargo workspace, layered bottom-up. Each crate depends only on those below it,
and the gpui-free core never imports gpui.

```
crates/
  model     shared domain types (the wire contract)
  store     ~/.hopper/ JSON persistence + OS keychain
  docker    Engine API client (hyper over unix/tcp/npipe) + every domain module
  apple     Apple Containers, driven through the `container` CLI
  engine    managed container engine + isolated desktop virtual machines
  machine   native ARM64 VM execution and device models (under development)
  migrate   Docker Desktop → Hopper migration
  host      the async service facade the UI calls
  mcp       the stdio MCP server (hoppermcp)
  app       the gpui + guise application (hopperdev / hopper)
```

The async Docker layer runs on a Tokio runtime; gpui has its own executor. The
two meet at one seam — `app::bridge` — which runs a future on Tokio and
delivers the result on the gpui main thread. Streaming calls (logs, stats,
events, exec) are cancelled by dropping the producer, so a closed view closes
its stream with no separate abort registry. This mirrors the
[tables](https://github.com/wess/tables) architecture.

## Run

```sh
cargo run -p app            # launch the app (dev binary: hopperdev)
cargo test                  # the whole suite
cargo clippy --all-targets  # lint
cargo run -p mcp            # the stdio MCP server
```

For development, run `scripts/build/lima.sh` once to fetch the pinned VM helper
and matching templates. `HOPPER_LIMA_BIN` overrides its path. The helper is bundled
automatically by `scripts/bundle.sh`; users do not need Homebrew or Lima installed.
`scripts/build/qemu.sh` fetches official Homebrew bottles and relocates the Windows
QEMU/TPM runtime, firmware, dependencies, and licenses without installing packages.
`cargo run -p engine --example machine -- create ubuntu "Desktop"` exercises
user VM creation; `start <id>` downloads and opens its viewer.
`cargo run -p engine --example vm -- start` exercises the managed VM directly.

An engine must be reachable. That can be Apple's `container`, Docker,
Podman, Colima, Rancher Desktop, or another Docker-compatible endpoint;
Hopper's first-run panel provisions its managed Docker VM. Virtual machines
remain available independently of the container engine.

For an explicit endpoint, set `DOCKER_HOST`; Hopper also understands Podman's
standard `CONTAINER_HOST` when `DOCKER_HOST` is not set. A saved engine choice
in Settings takes precedence over automatic discovery, while these environment
variables are useful for remote daemons and shell-driven workflows.

### Native VM development

The replacement Windows runtime uses Hypervisor.framework directly. It is not
connected to the app yet. Its firmware diagnostic boots ARM64 UEFI with a native
GIC, serial console, CFI flash, and firmware service calls; this does not establish
Windows installation or desktop support. The major release gates are in
[PRODUCT.md](PRODUCT.md).

On Apple silicon macOS, install LLVM, lld, and ACPICA for firmware compilation.
`scripts/build/firmware.sh` builds pinned TianoCore source into
`native/build/firmware/windows.fd` and `variables.fd`, with a provenance manifest
and upstream notices. It uses ArmVirt's generic firmware platform drivers and
does not run an emulator. `HOPPER_LLVM` can select the LLVM binary directory.

```sh
scripts/build/firmware.sh
cargo build -p machine --example firmware
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/firmware
target/debug/examples/firmware native/build/firmware/windows.fd /tmp/hopper.dtb native/build/firmware/variables.fd
```

The diagnostic selects the internal shell through the serial console, verifies
variable flash writes, and enforces a 30-second deadline. Its variable writes
remain in memory; no user VM is created or modified.

## Build a release

```sh
scripts/bundle.sh           # assemble + sign dist/Hopper.app
scripts/dmg.sh              # package dist/Hopper.dmg
```

CI builds and tests on Apple silicon macOS. Releases include a notarized macOS
DMG for Apple silicon; the scripts above handle signing and notarization with
the configured `CODESIGN_IDENTITY` credentials.

## Sponsor

♥ [Sponsor this project](https://github.com/sponsors/wess)
