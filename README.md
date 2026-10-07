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

- **Virtual machines** — new Ubuntu Desktop and Windows 11 ARM64 records use
  Hopper's native runtimes and dedicated viewers. macOS creation retains its prototype. Automatic image
  acquisition and installation preparation are implemented; complete native Windows
  installation and first boot remain unverified. A local installer is optional.
  Guest VMs live under `~/.hopper/machines`. Closing the native viewer keeps Windows
  running; quitting Hopper stops its owned Windows sessions. Windows snapshots,
  clones and accelerated graphics remain unfinished. Ubuntu first start prepares unattended
  installation and supports manually starting the installed system after a saved deployment
  result. Full installation, desktop readiness and guest tools remain unverified. Previous
  untagged Linux records retain their recovery path.
- **Agent access** — enabled for new VMs, revocable per VM. The MCP server creates
  and lists native Windows/Ubuntu records and captures Windows displays through the running app.
  Windows US key chords and pointer actions share input ownership with the viewer.
  Pause, resume and stop also use the owned runtime. Composed text, commands, files,
  snapshots and clones remain unavailable. Previous
  Linux/macOS prototypes expose guest operations; macOS input requires Accessibility
  permission inside the guest. Prototype snapshots and clones require a stopped VM;
  clones inherit agent access.

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

The replacement Windows runtime uses Hypervisor.framework directly. Windows creation,
listing, startup and dedicated viewing now use it in the app. Installation, first boot
and the rendered desktop remain unverified. Its firmware diagnostic boots ARM64 UEFI with a native
GIC, serial console, CFI flash, ACPI handoff, PCI configuration bus, and firmware
service calls. This does not establish Windows installation or desktop support. The major release gates are in
[PRODUCT.md](PRODUCT.md).

Native Windows agent screenshots use the running Hopper app's owned registry. Run
`hoppermcp` as the same macOS user, with the same `HOPPER_DIR` if overridden, then call
`vm.list` and `vm.screenshot` with the native VM ID. The viewer can be closed; the VM
must still be running and agent access must remain enabled. The private local service
accepts status, capture, connection-owned input and pause/resume/stop, rechecks persisted
access, and never launches another worker. `vm.input` supports US key chords such as `ctrl+alt+delete`
and normalized pointer actions; use `shift+=` for a plus sign. Another input owner
returns a busy error. Revocation, disconnect and inactivity release held keys/buttons
before ownership can change. Toggling access off and back on invalidates old agent
connections. `vm.pause` retains guest memory; `vm.resume` resumes hardware. `vm.stop`
stops hardware and waits for worker cleanup; it does not ask Windows to shut down.
Composed text, commands, files, startup, snapshots and clones remain unavailable remotely.
Inside-guest agent connections remain pending.

The direct Linux VZ foundation lives in `machine::vz`. It owns framework configuration
and asynchronous lifecycle on the main thread, with persistent identity/EFI variables,
raw storage, installer media and desktop devices. Host exposes native Linux preparation
and authorized lifecycle operations. Already-admitted VMs use a dedicated native viewer;
complete viewer verification remains unfinished. New Ubuntu creation and first startup
use the native path. New macOS records now explicitly select the native runtime through the
same validated record creator, preserving default agent access and optional local restore
media. Listing reports Setup required until native macOS installation is integrated. Existing
guest data remains on its recovery or migration path. A signed diagnostic
verifies real VZ hardware transitions with a temporary blank disk:

```sh
cargo +1.99.0 build -p machine --example vz
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/vz
target/debug/examples/vz
```

The `vzlinux` example additionally accepts an uncompressed ARM64 Image and initramfs.
Its signed live check boots the initramfs shell and repeats those transitions with
networking disconnected. The tested inputs came from the official
[Alpine 3.24.2 ARM64 virtual ISO](https://dl-cdn.alpinelinux.org/alpine/v3.24/releases/aarch64/),
verified against its SHA-256 `a57ba668b5f6b17a670fcf8e799d5d7fe43766ed086d6ce2927b0625bf43dbf6`.
The compressed EFI kernel was unpacked into its raw ARM64 Image before direct boot;
compressed kernels use the EFI path instead. See the
[ARM64 boot format](https://docs.kernel.org/arch/arm64/booting.html). This verifies a
diagnostic Linux kernel and userspace, not Ubuntu desktop installation or macOS support.

The `vzmac` example discovers Apple's supported restore-image metadata and configures
a macOS platform with its CPU/RAM requirements, independent identity and matching
auxiliary firmware. Creation publishes private state and its hardware binding together
and refuses to replace existing data. A signed live check validates configuration and
rejects insufficient resources and model mismatches. Local restore inspection uses the
framework to read media, checks the returned file URL, and rejects symlinks and
invalid files without changing their contents. It downloads no IPSW and performs
no successful installation or macOS first boot; manager/viewer integration remains
unfinished. The native installer controller holds exclusive main-queue VM access, reports
progress and completion, and requests cancellation on handle drop. A callback retains
hardware through asynchronous completion, and lifecycle commands remain blocked while
installation is active. The signed probe checks malformed-media installation failure;
successful installation and cancellation of a valid IPSW remain unverified.

A bounded VZ lifecycle bridge lets Tokio callers request operations while the native
owner stays on the VM queue. The signed `vz` diagnostic sends real start/pause/resume/stop
requests from a separate service thread, rejects retirement of active hardware and
checks that a cancelled queued start does not run. Native transitions retain the
framework machine until completion. Host now authorizes lifecycle requests against the
persisted VM record, keeps the original agent policy generation through dispatch and
completion, and holds the operation lock until queued cancellation or native completion.
The app installs the main-queue owner and wakes it on requests instead of polling while
idle. Linux preparation now stages a private sparse disk, identity and EFI variables,
publishing them together without replacing existing data. Main-thread admission binds
runtime ownership through pending SDK callbacks. A signed engine diagnostic verifies
admission, all four lifecycle states, agent revocation, runtime locks and persistent
identity/firmware reuse:

```sh
cargo +1.99.0 build -p engine --example admission
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/admission
target/debug/examples/admission /absolute/path/to/linux-arm64.iso
```

Native Ubuntu preparation now reports byte counts for download and cache verification,
followed by disk, installer and account preparation. Updates use bounded latest-value
channels through the native UI bridge. Interrupted downloads report the retained byte count;
resumed downloads continue from it, and cached verification does not claim a new download.
The stream carries the prepared VM to main-thread admission and preserves the automatic
installation watch after startup. Full app rendering of these phases remains unverified.

Native Ubuntu preparation automatically acquires the official Ubuntu 24.04.5 ARM64
desktop ISO when no local installer is supplied. The private cache resumes interrupted
downloads and verifies the pinned size and SHA-256 before publication and reuse. The
original agent policy applies throughout acquisition. Local ISO fallback checks only its
volume descriptor. The live `linuxmedia` diagnostic verifies official checksum metadata,
size, a bounded ISO HTTP range and insufficient-storage rejection. A complete official
download and installation remain unverified on this host. This path does not establish
desktop readiness or provide guest tools. Native-state records cannot fall back to previous
runtime operations. Listing
queries the app-owned queue for actual hardware state and pending-operation status,
without acquiring lifecycle/runtime locks. Queries recheck the original agent policy
generation; instances absent from the owner remain unavailable.
The app routes View and start/stop for already-admitted VMs through its native owner.
It caches one dedicated window per VM; closing the window hides it while hardware remains
owned. Display bindings retain runtime ownership and detach on drop, refuse duplicate
attachment and prevent retirement while attached. Host system hotkeys stay with the host.
Automatic guest-resolution changes remain disabled after an SDK reconfiguration callback
crashed during unattached-view teardown; window resizing does not yet change guest
resolution. The signed admission probe verifies display binding/lifetime on stopped
hardware, not real rendering, keyboard/mouse or window close/reopen. New Ubuntu records
persist an explicit runtime choice. Their first start acquires media,
prepares the VM, admits it on the main thread and opens the dedicated viewer after hardware
starts. First start now stages unattended EFI installation and guest account configuration;
a saved deployment result selects installed-system boot on the next Start. Full installation,
actual accounts, desktop readiness and guest tools remain unverified. A local Ubuntu Desktop ARM64 ISO is available as a fallback. Prepared,
unowned records show Ready to start only when operation/runtime locks are free; this does
not assert an installed or usable desktop. Previous untagged records retain their recovery
path, and mismatched guest/runtime choices are rejected. macOS creation still uses the
prototype. MCP routing, full guest display/input checks and signed app integration remain
unfinished.

Native Linux admission attaches a virtio network device using Apple's NAT. Its locally
administered MAC address derives from the saved VM identity, so repeated admission keeps
the same address. A disconnected configuration is available in the core; app settings,
live switching and localhost port forwarding remain unfinished. A signed diagnostic
verifies guest DHCP, DNS, downloading public Alpine release metadata and a disconnected
guest link. It uses a disposable initramfs derived from verified Alpine media:

```sh
python3 scripts/check/network.py /path/to/initramfs-virt /path/to/network.gz
cargo build -p machine --example vznetwork
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/vznetwork
codesign --verify --strict target/debug/examples/vznetwork
target/debug/examples/vznetwork /path/to/uncompressed/Image /path/to/network.gz
```

Ubuntu provisioning plans now generate separate normal and administrator password hashes
and NoCloud `user-data`/`meta-data` in a bounded ISO9660/Joliet image. Linux keychain entries
are separate from Windows entries, and retries reuse generated guest credentials. Prepared
native VMs can attach this seed read-only; its private files live until hardware and pending
callbacks release ownership. The signed diagnostic also verifies mounting the seed and
reading its configuration in a real guest:

```sh
cargo run -p engine --example linuxseed -- /absolute/path/to/newseed.iso
target/debug/examples/vznetwork /path/to/uncompressed/Image /path/to/network.gz /absolute/path/to/newseed.iso
```

The app now selects unattended preparation for first start, attaches provisioning media and
passes the unattended boot flag through a privately staged EFI installer. Staging validates
the ARM64 kernel and desktop installation sources, clones the ISO on APFS, then changes only
the existing GRUB configuration. Copy fallback requires enough space. The downloaded cache
stays unchanged. Existing written disks reject unattended preparation, and admitted installer
hardware cannot start a second time. Attempt-scoped completion markers and a private
journal support automatic system startup after successful installer shutdown, with a manual
Start path after an app restart. Explicit Stop requests cancel preparation and automatic
startup; agent watches retain their original access policy. Seed delivery, schema checks
and staged boot flags do not prove
installed users, successful Ubuntu installation, guest tools or a usable desktop.

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
Windows guest input is checked separately by the WinPE diagnostic below.

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
`drvload` results for both drivers (exit code 0). PCI MSI-X tables now connect
queue vectors to the native MSI frame advertised through ACPI and FDT. Storage
and both input controllers reach driver-ready status (`0x0f`), and DiskPart sees
the 604 MB diagnostic image as an online disk. It exposes no volumes: installation
media mounting, a writable installation target and a complete Windows installation
remain unfinished.
A separate run with `--check-guest-input` on this boot-only diagnostic media typed
`echo hopper windows input verified` through the Windows keyboard driver. The
exported frame showed the command and its output at the WinPE prompt; 144 input
events completed and 38 native MSI messages were delivered without device faults.
Use this flag only with the driver diagnostic image, which opens a command prompt.
`--check-guest-disk /tmp/newtarget.img` creates a separate 64 MB sparse target,
refuses an existing path and types a DiskPart partition check into that prompt.
A live 90-second run showed a new 62 MB primary partition; the retained backing
file contained the corresponding partition table. The target completed 76 storage
requests without a device fault. This verifies guest disk writes, not a formatted
Windows installation or installation-media mounting. Input and disk checks are
separate modes; allow `--seconds 90` for the disk check.
The native device path also has a read-only optical transport over Virtio SCSI.
It exposes 2048-byte sectors, inquiry/capacity/TOC/configuration responses, bounded
reads, write protection and checked control/request queues. Sense/CDB sizes reset
to their defaults; unnegotiated bidirectional requests and bad targets return
transport errors. The event queue remains inactive because hotplug is not offered.
The diagnostic builder includes the signed ARM64 `vioscsi` package. Live probes
load it successfully, and Windows PnP reports its controller as Started. Firmware
reads the optical image, but Windows still exposes no CD volume, including after
an explicit DiskPart rescan. This path is not ready for installation.
Use `--optical /path/to/installer.iso --check-optical` to repeat the volume check
with the boot-only diagnostic image, or `--check-driver` for the controller's
Windows PnP status. Optical statistics report per-queue counts, command types,
check conditions and rejected LUN/transfer metadata without exporting guest buffers.

The firmware probe reports PCI accesses after its exit callback, each device's
command/BAR state, Virtio status and delivered native MSI messages. The Hypervisor
probe verifies frame identification, rejected overlapping/misaligned regions,
invalid message targets and pending interrupts alongside wired interrupts.
Tests cover vector selection, masks, pending replay, read-only capability fields,
bus-master gating and reset. Frame export preserves valid black displays rather
than discarding their diagnostic evidence.

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

Unattended Ubuntu setup now emits attempt-scoped serial phases for installation,
deployment completion and failure. The runtime drains console noise without saving guest
logs, and persists a private bounded journal that status checks can read after restart.
Deployment completion does not establish desktop readiness; full Ubuntu installation
remains unverified.

Launch now selects system boot from a saved deployment result and preserves that intent
across a failed handoff or app restart. Pressing Start on a completed, stopped installer
retires its hardware and reuses the dedicated window for the replacement VM. The installed
system omits installation and seed media, while retaining its disk, platform identity and
EFI variables. Successful installer shutdown now triggers the same handoff automatically.
The library exposes Pause and Resume for owned native Linux and Windows machines, including
a running Ubuntu installer. Controls wait for acknowledged lifecycle completion, recheck
authorization and refresh status; paused installation time stays outside the watch timeout.
Native window rendering and desktop application continuity remain unverified.
Explicit Stop requests and agent-access revocation suppress it, and runtime generations
prevent stale callbacks from replacing newer hardware. Paused installation time does not
consume the automatic watch timeout. Incomplete installations remain on the recovery path.
A signed diagnostic verified real Linux guest shutdown, hardware replacement and cancellation
using synthetic disk data and completion markers. Full Ubuntu installation, installed-system
boot, desktop readiness and the dedicated window behavior remain unverified.
