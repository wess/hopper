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
GIC, serial console, CFI flash, ACPI handoff, PCI configuration bus, and firmware
service calls. This does not establish Windows installation or desktop support. The major release gates are in
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

The diagnostic selects the internal shell through the serial console, uses
`acpiview` to verify the installed CPU/interrupt/timer/power/PCI tables, checks reserved
memory, PCI enumeration and variable flash writes, and enforces a 30-second deadline.
Its variable writes remain in memory; no user VM is created or modified.

The PCI root describes one bus, a 256 MiB memory aperture, a separate ECAM
reservation, and four shared interrupt lines. CI uses ACPICA to interpret its
resource buffers and verify every interrupt route. Disk I/O remains under development.

The storage core handles split Virtio queues and file-backed sector reads, writes,
flushes and disk identification. It checks DMA ranges, descriptor chains, ring
wraparound and writable completion lengths. The separate native storage probe uses
an ARM guest to submit a request and read its response from mapped RAM, then checks
the interrupt signal. It opens its supplied test image read-only.

```sh
cargo build -p machine --example storage
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/storage
target/debug/examples/storage /path/to/sector-aligned-test.img
```

The modern PCI transport exposes both MMIO and configuration-window access, feature
negotiation, queue setup, reset, and legacy interrupt acknowledgement. The firmware
diagnostic can discover a read-only disk and verify a file through UEFI's FAT driver:

```sh
python3 scripts/check/storage.py /tmp/hopperstorage.img
target/debug/examples/firmware native/build/firmware/windows.fd /tmp/hopper.dtb native/build/firmware/variables.fd /tmp/hopperstorage.img --check-storage
```

The fixture builder creates a new image and refuses to overwrite an existing file.
This verifies UEFI storage access, not Windows installation or desktop support.

The 2D GPU uses the same modern PCI transport, with independent control and cursor
queues. It creates bounded resources, attaches validated guest-memory buffers,
transfers rectangular pixel updates, and publishes RGBA scanouts and cursor images.
Tests cover fragmented DMA, format conversion, cropped displays, fence responses,
invalid requests, allocation limits, queue selection, notifications, and reset.
The live firmware diagnostic initializes UEFI graphics and exports its rendered
shell while also checking storage:

```sh
target/debug/examples/firmware native/build/firmware/windows.fd /tmp/hopper.dtb native/build/firmware/variables.fd /tmp/hopperstorage.img --check-storage --check-graphics --frame /tmp/hopperframe.ppm
```

Frame export refuses to overwrite a file. This verifies firmware graphics and disk
access together; Windows display drivers and 3D acceleration remain unverified.

Hopper's derived GOP driver also exposes a 32-bit BGRX physical framebuffer.
Its pages use UEFI reserved memory, so the OS cannot reclaim them after firmware
handoff. A versioned mailbox at GPU BAR0 offset `0x3800` publishes the address,
dimensions and stride. Capture validates and samples this guest RAM while every
CPU is paused. The layout survives Virtio reset; a new Virtio scanout takes over
when an OS driver selects one. Tests cover direct writes, reset, row padding,
invalid layouts and driver takeover. This is the pre-driver display path,
not accelerated Windows graphics.

Native keyboard and absolute-pointer devices expose guest capabilities and bounded
event queues through PCI. Input waits for guest receive buffers and bus mastering;
release-all discards unsent transitions and releases delivered keys or buttons.
The firmware profile connects its Virtio keyboard to the UEFI console. Add
`--check-input` to type a check command through that keyboard and deliver pointer
events. This check allows 60 seconds and paces keys because the upstream firmware
driver retains one key per poll. Pointer interaction with a guest application and
Windows input drivers remain unverified.

`--boot-media` is a separate installer diagnostic: it maps 4 GiB of guest RAM,
attaches keyboard/pointer controllers, and sends Enter after Microsoft's ARM64
CD boot program loads. Supply the cached installer as its disk argument and use
`--frame` to preserve the display when the 60-second probe fails. The cached ISO
is opened read-only. It now passes Microsoft's CD boot program and the UEFI
handoff. A live 60-second run rendered Windows 11 Setup's storage-driver dialog
through the physical framebuffer, and shorter runs reached user code and kernel
system calls. Setup still needs a guest storage driver to access its installation
media. The physical framebuffer replaces the previous `PixelBltOnly` mode;
installation, desktop integration and accelerated display drivers remain unverified.
The probe requests a native CPU exit every 20 milliseconds so the owner thread
can deliver input even when guest execution makes no device accesses.
`--seconds` accepts a bounded diagnostic duration from 3 to 300 seconds.

`scripts/build/windowsprobe.py` prepares a separate, boot-only Windows PE driver
diagnostic from the cached installer and an extracted ARM64 Virtio driver package.
It validates driver executable architectures, retains the package license and records
driver hashes. It adds a WinPE startup script that runs `drvload` for storage/input,
prints each exit code and lists visible disks/volumes. It does not launch Setup or
change the source installer. Windows remains responsible for accepting driver catalogs;
the diagnostic does not disable signature checks. It requires `bsdtar`, `wimlib-imagex`
and a UDF-capable `mkisofs` to preserve the installer's filesystem format.
A live run with the Virtio Windows 0.1.302 ARM64 package reported successful
`drvload` results for both drivers (exit code 0), but DiskPart found no disks or
volumes. Package acceptance does not establish working device enumeration, disk
access or input; those remain under development.

```sh
python3 scripts/build/windowsprobe.py /path/to/installer.iso /path/to/virtio-win/drivers/by-driver /tmp/hopperdrivers
target/debug/examples/firmware native/build/firmware/windows.fd /tmp/drivers.dtb native/build/firmware/variables.fd /tmp/hopperdrivers/drivers.iso --boot-media --seconds 90 --frame /tmp/drivers.ppm
```

CPUs created after the native GIC expose the framework's virtual PMUv3, which
Windows boot code requires. Guest OS debug-lock status and access are emulated
per CPU with the architectural cold-reset state; unknown system registers fail
explicitly. A separate native probe verifies the advertised version, an advancing
cycle counter, and the guest lock/unlock instructions:

```sh
cargo build -p machine --example performance
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/performance
target/debug/examples/performance
```

`cargo run -p machine --example acpi -- /tmp/hopperacpi` exports the handoff and
individual tables for ACPICA inspection. TPM is still missing. The native runtime,
storage, and graphics have not been integrated into the app.

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
