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
quit stops the engine. Bundles include the container VM helper, Docker CLI, Compose,
Hopper's native Windows worker and firmware, and Windows installation tools.

## Features

- **Virtual machines** — new Ubuntu Desktop and Windows 11 ARM64 records use
  Hopper's native runtimes and dedicated viewers. New macOS records use native VZ setup.
  Automatic image acquisition and installation preparation are implemented for all three guest families.
  Native Windows acquisition, unattended deployment and clean installer shutdown are live verified.
  Installed Windows completes first boot and renders the signed-in desktop. A signed isolated
  app verifies dedicated-window rendering, pause/resume, close/reopen without restarting,
  pointer targeting at the Windows sign-in screen, and guest-initiated shutdown.
  Short keypresses produce the expected character count while the viewer polls; Shift case and
  release, Control selection and deletion are live verified. Screenshot transport latency is reduced,
  but responsiveness at larger resolutions, broader held-input behavior and Windows guest tools
  need further checks.
  A local installer is optional.
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
`scripts/build/windows.sh` fetches and relocates Windows installation tools and drivers,
including their provenance and licenses, without installing packages.
The Windows build also requires LLVM clang, llvm-dlltool and lld-link to compile Hopper's
freestanding ARM64 setup handshake helper; its MIT notice ships in the bundle and setup media.
Desktop VMs use Hopper's native runtimes; QEMU and swtpm are not bundled. Previous Windows records require
explicit migration and retain their disks without being started by the old helper.
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
listing, startup and dedicated viewing now use it in the app. Official acquisition and WinPE
deployment are verified. Clean WinPE shutdown now precedes installed-system startup, which
passes the previous `msseccore.sys` recovery failure and reaches the signed-in desktop.
Read-only inspection confirms the installed driver matches the official image. Dedicated
viewer input and guest tools remain unverified or unfinished. Its firmware diagnostic boots ARM64 UEFI with a native
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
media. Listing reports Setup required before macOS installation. Existing
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

Native macOS restore inspection now stages local media privately before asking the framework
to inspect it. APFS clones retain independent blocks; copy fallback checks free space and
original authorization throughout copying. Staging checks source identity, size and timestamps,
rejects symlinks/empty files, and retains original policy and explicit-stop cancellation through
SDK completion. The SDK callback keeps staging data and operation ownership alive after caller
cancellation. Dropping completed inspection releases its private copy. The inspected result
now prepares private sparse storage under runtime ownership and can admit native hardware
on the main thread. Publication stores machine identity, restore
build/version/requirements and matching auxiliary firmware together. Retry validates and
reuses those bindings; caller cancellation cleans unpublished storage. SDK hardware and
displays retain runtime and media ownership. Normal Start rejects macOS hardware until
native installation reports success; failed installation keeps the guard in place.
Inspected preparation can now start installation through the owned main queue and return
a progress/completion handle. SDK callbacks retain hardware, media and original operation
authorization through cancellation acknowledgement. Dropping or cancelling a handle,
revoking access, requesting Stop or exceeding the bounded deadline cancels installation;
failed or cancelled completion cannot enable Start. Installation remains exclusive through
queue completion, including after the SDK callback. Installation now saves a private,
platform-bound attempt before starting the SDK and commits installed state only on authorized
successful completion. Interrupted installation or written disks without a receipt require
recovery, preserving guest data. System preparation validates the saved receipt, identity,
capacity, hardware and requirements, then admits installed hardware without restore media.
Listing distinguishes setup, recovery and installed readiness; it does not claim a verified
desktop. Library Start now inspects local restore media, prepares Mac hardware, opens its
dedicated viewer, streams bounded installation progress and starts the installed system after
authorized successful completion. Cancel setup invalidates the original stop intent without
waiting for its retained operation lock. Installed records reopen through saved platform
preparation without restore media. Direct installation wakes the queue owner. With no local
restore image, first start discovers the latest supported image through Apple’s framework,
checks official HTTPS metadata and downloads into a private cache. Resume binds the URL,
exact size and strong ETag; conditional ranges reject changed responses. Each completed cache
reuse rechecks its SHA-256, and staged SDK inspection must match the discovered build, version,
hardware model and resource requirements. The SHA-256 is a local cache integrity receipt,
not an Apple-published checksum. Downloads preflight remaining space and preserve partial
bytes across interruption; original authorization is checked while waiting for network data.
Full IPSW acquisition, valid-media installation, installed macOS boot and actual viewer
rendering/input remain unverified. A signed live diagnostic verifies official metadata and a
1 KiB conditional range. An isolated signed app verifies automatic first-start discovery and
low-storage rejection before image transfer or hardware creation.

The `vzmac` example discovers Apple's supported restore-image metadata and configures
a macOS platform with its CPU/RAM requirements, independent identity and matching
auxiliary firmware. Creation publishes private state and its hardware binding together
and refuses to replace existing data. A signed live check validates configuration and
rejects insufficient resources and model mismatches. Local restore inspection uses the
framework to read media, checks the returned file URL, and rejects symlinks and
invalid files without changing their contents. It downloads no IPSW and performs
no successful installation or macOS first boot; live viewer verification remains unfinished.
The native installer controller holds exclusive main-queue VM access, reports
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
download, independent checksum verification, unattended installation and automatic installed-system
handoff have been verified in an isolated signed app profile. Ubuntu reached its desktop as
the normal `hopper` user (UID 1001, without sudo). Administrator login and guest tools remain
unverified. Native-state records cannot fall back to previous
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
a saved deployment result selects installed-system boot on the next Start. Live Ubuntu installation,
automatic system handoff, desktop rendering and normal-user identity are verified; administrator
login and complete guest tools remain unfinished. A local Ubuntu Desktop ARM64 ISO is available as a fallback. Prepared,
unowned records show Ready to start only when operation/runtime locks are free; this does
not assert an installed or usable desktop. Previous untagged records retain their recovery
path, and mismatched guest/runtime choices are rejected. New macOS creation and library
setup use the native path. MCP routing, full guest display/input checks and signed app integration remain
unfinished.

Native Linux and macOS admission configure a virtio network device with Apple's NAT.
Its local unicast address derives from the persisted VM identity and stays stable across
admission retries. The VM library saves a per-VM NAT/disconnected choice for native Linux
and macOS guests. Shut down a guest before changing this setting; the new choice applies
on its next start. Stopped owned hardware is detached and retired before saving the choice.
Private settings reject malformed, foreign, symlinked or unbounded data instead of silently
reconnecting. Operation/runtime ownership excludes concurrent policy changes during preparation
and admission. Signed SDK probes verify both persisted modes, readmission identity and
blocked policy replacement while hardware is owned. Actual macOS guest traffic, live attachment
switching and localhost forwarding remain unverified or unfinished.
[Apple's NAT attachment](https://developer.apple.com/documentation/virtualization/vznatnetworkdeviceattachment)
routes guest traffic through the host without requiring the bridged-network entitlement.

For Linux, a signed diagnostic verifies guest DHCP, DNS, downloading public Alpine
release metadata and a disconnected
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
Deployment completion alone does not establish desktop readiness. An isolated app run also
verified the installed Ubuntu desktop after automatic handoff.

Launch now selects system boot from a saved deployment result and preserves that intent
across a failed handoff or app restart. Pressing Start on a completed, stopped installer
retires its hardware and reuses the dedicated window for the replacement VM. The installed
system omits installation and seed media, while retaining its disk, platform identity and
EFI variables. Successful installer shutdown now triggers the same handoff automatically.
The library exposes Pause and Resume for owned native Linux and Windows machines, including
a running Ubuntu installer. Controls wait for acknowledged lifecycle completion, recheck
authorization and refresh status; paused installation time stays outside the watch timeout.
Ubuntu installer and installed-desktop rendering are verified. Desktop application continuity
across lifecycle operations remains unverified.
Explicit Stop requests and agent-access revocation suppress it, and runtime generations
prevent stale callbacks from replacing newer hardware. Paused installation time does not
consume the automatic watch timeout. Incomplete installations remain on the recovery path.
A signed diagnostic verified real Linux guest shutdown, hardware replacement and cancellation
using synthetic disk data and completion markers. A separate isolated app run verified full
Ubuntu installation, automatic installed-system boot and desktop rendering. Closing and reopening
the dedicated window while the installer ran preserved its runtime.

The native VZ core also accepts named shared folders with explicit read-only or read-write
access. In the VM library, **Add folder…** opens the native folder picker, then lets you name
the guest folder and choose whether to allow changes. New shares default to read-only.
Folder selection, access changes and removal apply on the next start and require a stopped VM.
Grants are saved privately with the selected folder's filesystem identity. A missing or replaced
folder blocks startup until you remove its grant and select the intended directory again.
Agents cannot grant additional host folders through MCP. Linux uses the `hopper` virtiofs tag. Mac configurations use
[Apple's guest automount tag](https://developer.apple.com/documentation/virtualization/vzvirtiofilesystemdeviceconfiguration/macosguestautomounttag).
The core checks the selected directory's canonical path and filesystem identity before
configuration and startup, retaining its handle through hardware, viewer and callbacks.
Replacing the host path while running may make the share unavailable rather than redirect it.

New Ubuntu provisioning installs a root-owned mount helper and a boot service. The service
mounts the `hopper` device at `/mnt/hopper` with `nosuid,nodev`, and skips mounting when no
shared-folder device exists. A `Shared` link in the normal user's home points to a guest-local
index at `/run/hopper-shares`, containing links to each named directory under the mount.
This avoids file-manager metadata queries against Apple's synthetic virtiofs root, which
appeared as a broken link in the installed Ubuntu desktop. The index is root-owned and
bounded to 16 entries; unexpected contents are preserved and rejected.
The helper checks an existing mount before reuse and refuses a symbolic-link mount point.
Existing guests do not receive these tools automatically.

A disposable signed Linux diagnostic runs that exact helper and verifies no-device startup,
repeated mounting, rejection of an unrelated existing mount, named reads and writes as root
and an unprivileged user, read-only
enforcement, absolute and relative symlink containment, rejection of a replaced folder
before startup, and no redirection to a replacement while running:

```sh
clang --target=aarch64-linux-gnu -nostdlib -static -fno-stack-protector -fno-builtin -O2 \
  --ld-path=/opt/homebrew/bin/ld.lld scripts/check/identity.c -o /tmp/hopperidentity
python3 scripts/check/sharing.py /path/to/initramfs-virt /path/to/sharing.gz /tmp/hopperidentity
cargo build -p machine --example vzsharing
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/vzsharing
codesign --verify --strict target/debug/examples/vzsharing
target/debug/examples/vzsharing /path/to/uncompressed/Image /path/to/sharing.gz
```

The diagnostic identity helper is a Linux ARM64 fixture, not an installed guest tool.
The installed Ubuntu service activated successfully; the ordinary user read a shared fixture
and was refused a write to its read-only grant. A local-index prototype also opened that folder
in Files. The updated provisioning helper still needs verification in a freshly installed
desktop. Existing-guest tool updates, macOS guest access, Windows sharing and complete desktop
sharing verification remain unfinished.

Native Linux and macOS guests also have a persisted **Speakers** switch in the VM library.
It defaults on, requires a stopped VM with hardware ownership released, and applies on the
next start. Enabled hardware has one output-only virtio sound stream connected to the host;
disabled hardware has no audio device. No microphone stream is configured. Microphone input
and Windows audio remain unavailable. Invalid or foreign settings block preparation instead
of silently restoring enabled speakers.

The signed audio diagnostic checks a real Linux guest's playback device and absence of capture
streams in enabled/disabled configurations, using the matching verified Alpine module image:

```sh
bsdtar -xOf /path/to/verified-alpine-virt.iso boot/modloop-virt > /path/to/modloop
python3 scripts/check/audio.py /path/to/initramfs-virt /path/to/audio.gz
cargo build -p machine --example vzaudio
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/vzaudio
codesign --verify --strict target/debug/examples/vzaudio
target/debug/examples/vzaudio /path/to/uncompressed/Image /path/to/audio.gz /path/to/modloop
```

This verifies device discovery, not audible playback or installed-desktop sound. Signed Linux
and Mac admission probes also check the persisted enabled/disabled audio-device count and
reject setting changes while hardware is owned. Full desktop audio verification remains pending.


Native Linux hardware also includes one virtio socket device for guest-tool transport.
The core can attach one guest-initiated listener on port 6200, retaining one connection
on the VM queue. Reads and writes are nonblocking and limited to 64 KiB per call; descriptors
are close-on-exec and suppress SIGPIPE. The original authorization is checked on acceptance
and before/after I/O. Revocation closes the connection; revoked read data is cleared before
return. Paused or stopped hardware cannot serve I/O. Closing the listener removes its SDK
registration and releases retained runtime ownership. An attached listener blocks retirement.

A signed disconnected Linux diagnostic verifies actual UID 1001 guest-to-host socket data,
bounded reads/writes, duplicate-listener rejection, pause/resume disconnection, original-policy
off/on rejection, ownership lifetime and retirement exclusion. The ARM64 socket executable
is a disposable diagnostic fixture, not a guest command service:

```sh
clang --target=aarch64-linux-gnu -nostdlib -static -fno-stack-protector -fno-builtin -O2 \
  -Wall -Wextra -Werror --ld-path=/opt/homebrew/bin/ld.lld scripts/check/socket.c -o /tmp/hoppersocket
python3 scripts/check/socket.py /path/to/initramfs-virt /path/to/socket.gz /tmp/hoppersocket
cargo build -p machine --example vzsocket
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/vzsocket
codesign --verify --strict target/debug/examples/vzsocket
target/debug/examples/vzsocket /path/to/uncompressed/Image /path/to/socket.gz /path/to/modloop
```

The modloop must come from the same verified Alpine image as the kernel and initramfs.
Guest service installation, bounded normal/admin jobs, file framing, app/MCP routing and
macOS/Windows guest transports remain unfinished. Native VZ viewer automation currently
loses Shift and Control modifiers; physical keyboard modifier behavior is unverified.


The guest-only `guest` crate now supplies the Linux job runner. Requests contain a literal
executable and argument list, an explicit normal or administrator role, an optional absolute
guest working directory, and a 1–600 second timeout. Executable names use a fixed guest search
path; absolute paths follow the selected guest OS. There is no implicit shell or inherited
service environment. Starting jobs have no execution identity; after child admission, results
report the verified launch UID, GID and supplementary groups. Normal jobs run as the guest's
`hopper` account without capabilities or setuid elevation; administrator jobs run as root.

The root service owns a private cgroup-v2 directory through an exclusive flock. Each job joins
its own cgroup before privilege reduction. Pinned directory handles prevent path replacement
from redirecting process-tree cancellation. At most two jobs run concurrently; the last 16
results retain at most 64 KiB for each output stream. Excess output is drained and marked
truncated. Cancellation, timeout and manager drop kill all descendants, including new sessions.
The job remains unfinished and retains its slot until the child is reaped, the cgroup is empty,
and bounded cleanup succeeds. Unknown previous groups reject startup without deletion.

A signed disconnected Linux diagnostic runs the actual runner and verifies normal/admin
identities, supplementary groups, no normal capabilities, rejection of a root-owned setuid
executable, literal arguments, startup failure, output limits, concurrency/history limits,
nested cgroup cleanup, detached-descendant cancellation, timeout and manager drop/reopen:

```sh
rustup target add aarch64-unknown-linux-musl
CARGO_PROFILE_RELEASE_STRIP=symbols \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/opt/homebrew/bin/ld.lld \
  cargo build -p guest --example jobs --target aarch64-unknown-linux-musl --release --locked
python3 scripts/check/jobs.py /path/to/initramfs-virt /path/to/jobs.gz \
  target/aarch64-unknown-linux-musl/release/examples/jobs
cargo build -p machine --example vzjobs --locked
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/vzjobs
codesign --verify --strict target/debug/examples/vzjobs
target/debug/examples/vzjobs /path/to/uncompressed/Image /path/to/jobs.gz
```

These checks exercise a disposable guest, not an installed Ubuntu command service.
Installed authenticated socket transport, service provisioning, app/MCP routing,
file transfer and macOS/Windows job runners remain unfinished. CI now explicitly checks
Linux musl guest code; a macOS workspace check alone does not compile that implementation.

Working-directory changes now occur after cgroup admission and privilege reduction, so normal
commands cannot enter a private root directory through service privileges. Cancellation during
startup takes priority over an execution-failure classification. The updated diagnostic adds
both checks. These latest changes cross-compile and pass local lint/model checks, but their live
rerun is unverified: the current execution profile reports virtualization unavailable.

The shared HPG1 job codec now frames start, poll and cancel requests plus job/error replies.
It handles fragmented 64 KiB I/O chunks, limits requests to 256 KiB and replies to 1 MiB,
rejects pipelined frames and closes permanently after malformed framing or invalid fields.
Both 64 KiB binary output streams fit the reply bound. Five parser tests cover fragmentation,
oversized headers, schema rejection, escaped arguments and maximum output. Sequence numbers
identify replies; this plain codec does not authenticate them. This codec is
not an exposed service. Guest errors now retain at most 512 UTF-8 bytes. The updated guest
diagnostic round-trips job replies and checks a Unicode launch failure; live verification of
these additions remains pending.

A separate authentication layer now performs a three-message mutual handshake using a
per-boot 32-byte secret, fresh host/guest nonces and HMAC-SHA256. Its proofs and directional
frame keys bind the VM identity, runtime generation, guest OS and original agent-policy
generation. Serial sequences reject stale replies, replay, reflection and pipelining.
Malformed input closes the session and erases its owned keys. Incremental reads enforce the
same frame bounds and verify authentication before decoding job fields.

The Linux guest dispatcher accepts completed guest sessions and routes authenticated
start/poll/cancel requests to the job runner. Closing or dropping it, including after a forged
frame, cancels its process trees. The disposable guest diagnostic now includes authenticated
normal/admin jobs and forged-frame cleanup; those live checks remain unverified. Secret
provisioning, daemon installation, host policy checks, installed service and app/MCP routing
remain unfinished. This authentication layer does not encrypt the dedicated VM transport.

The Linux `hopperguest` binary now reads fixed-size boot credentials from `/run/hopper/boot`
and connects to host CID 2 on virtio-socket port 6200. It requires root and verifies a guest
local CID before preparing jobs. The credential directory must be private and root-owned;
the single-link regular file must be root-owned, mode 0400 and exactly 61 bytes. No-follow
opens prevent substituting a symlink or special file. Three material-codec tests cover valid
authentication, malformed/truncated documents and invalid boot identity.

Its nonblocking socket loop bounds each read/write to 64 KiB, bounds connect and partial
messages/replies to five seconds, and gives authentication ten seconds. It checks stop intent
at most every 100 ms and retries connections after two seconds. SIGTERM/SIGINT and disconnect
cancel jobs; teardown waits for confirmed cleanup before reconnecting or exiting normally.
Failed cleanup keeps ownership. A systemd unit is included at
`crates/guest/assets/hopperguest.service` and included in new installer seeds. ARM64 builds
and host/Linux lint pass. Socket exchange, signal handling, protected credential-file checks
and systemd activation remain live-unverified, as does the installed guest service.

The host-side `machine::guest` connection now authenticates and sends serial job requests
over a nonblocking transport. It reads/writes at most one 64 KiB chunk per tick, checks
original authorization before and after work, and checks it again when publishing a cached
reply. Timeouts, malformed replies, idle EOF, unsolicited bytes and revocation discard
buffered results and drop the transport owner. Busy requests leave the current request intact.
The VZ queue can create a client for matching owned Linux boot material; that client remains
bound to its VM queue. Socket callback state retains runtime ownership through queued SDK
callbacks. Five portable peer tests and the client's queue-thread compile-fail check pass.
These checks do not prove a live VM exchange. Live boot-secret delivery, external MCP
verification and live callback-lifetime verification remain unfinished.

The Linux NoCloud builder can now add the public ARM64 daemon and its root-owned systemd
unit while preserving accounts, shared-folder files and startup commands. It enables the
unit without starting it. Seed user data is bounded to 2 MiB, including the base64 executable;
metadata remains bounded to 4 KiB. Independent ISO extraction verifies the exact binary,
ownership declarations and existing configuration. Authentication keys are excluded from
installer media, which ordinary guest users can read. Automatic Ubuntu preparation loads
and validates the bundled tools before staging installer media, then adds them to its seed.
The service now runs private credential bootstrap before its daemon. Linux system hardware
admission attaches the private port; guest activation remains live-unverified.
Fresh installed-guest verification remains pending.

`bash scripts/build/guest.sh` builds the static ARM64 musl daemon with a configured `ld.lld`
linker and an installed Rust target. It publishes only the daemon, service and checksum
manifest under `native/build/guesttools`; previous guest kernel media stays separate.
Bundling rebuilds this package and verifies the copied `Contents/Resources/guesttools`
inventory. Validation rejects dynamic linking, invalid ELF entry/segment bounds, wrong
architecture, extra files, symlinks and changed checksums. The real helper package and two
asset validation tests pass locally. The runtime loader checks the exact resource inventory,
bounded no-follow file reads, checksums, static ARM64 executable and service matching this
build. App and sidecar lookups stay inside their app resources. Four loader tests pass locally,
including independent ISO extraction of the real built daemon through production provisioning.
CI now builds and exercises this package; that new workflow has not run yet. The full signed
app bundle, dependency license inventory, installed service and fresh guest activation remain
unverified or unfinished.

The VZ core now has a private boot-channel factory. It writes the exact HGB1 document to
an anonymous pipe, closes its writer and checks original authorization before and after
preparation. A consuming hardware factory attaches that input to a named `hopper.boot`
virtio console port with `isConsole=false`; guest output goes to `/dev/null`, separate from
diagnostic serial output. Framework handles own duplicated close-on-exec descriptors.
Hardware retains the boot identity/generation, rejects mismatched command material and
requires replacement before reusing its consumed input on a later start. Two pipe tests
verify exact material, EOF, descriptor flags and authorization rejection. This is a host
foundation: Linux system admission now calls it. Actual SDK port delivery and
unprivileged guest rejection remain unverified.

The Linux helper's fixed `--bootstrap` mode discovers the unique named port through sysfs,
checks its character-device identity and root-only mode, and reads exactly 61 bytes with a
180-second deadline and cancellation checks. The temporary key buffer is wiped on drop.
It publishes a root-owned 0400 file under private tmpfs `/run/hopper`, using exclusive
directory ownership and no-replace publication. Existing credentials are validated;
unknown or interrupted state is preserved and rejected. The systemd unit runs bootstrap
before the daemon, and installer seeds add a root-only udev rule. Two device-name/number
tests, the rebuilt static helper package and updated ISO/resource checks pass locally.
Linux/host lint also passes. Actual device access, private publication, signal cancellation,
systemd activation and end-to-end key delivery still need live verification.

Linux system admission generates a fresh random boot key in memory and uses the owner's
next hardware generation. Insertion checks that the private boot identity/generation match
the admitted VM. Installer hardware retains its existing path. The owner keeps the material
for fresh mutually authenticated command connections; each handshake has a new nonce and
the caller's original policy scope. A new model test verifies repeated handshakes preserve
the material and independently authenticate both scopes. Library Start retires stopped,
previously started private-channel hardware and prepares a fresh boot; viewer detachment
and runtime exclusion still precede retirement. Direct lifecycle restart rejects consumed
input. App check, core/app Clippy and protocol/provisioning checks pass locally. The local
Metal compiler required an explicit module-cache path within writable temporary storage.
These checks do not prove SDK admission, GUI restart, guest credential receipt or command
execution. External MCP and live lifecycle verification remain unfinished.

The app-owned queue now drives up to 16 authenticated guest connections, with one pending
request per VM. Connections persist between start/poll/cancel calls so guest jobs survive
individual replies. Each 50 ms drive checks original authorization, hardware generation and
caller cancellation before and after transport work and before reply publication. Pause/stop,
revocation, malformed replies and disconnected callers close the connection; guest service
closure cancels its session's jobs. The queue keeps the app's owner pump active independently
of viewer windows. `Host::virtual_guest_request` routes native Linux requests through the
service; session checks retain policy identity without retaining the request's command lease.
Command leases serialize requests separately from lifecycle operations, allowing Stop while
a request waits for the guest. Three queue tests verify queued revocation, cancelled/unowned
requests and early validation; the existing five authenticated peer tests also pass. Strict
app/core Clippy passes locally. These checks do not prove live command execution, persistent
job polling, stop/lease recovery or guest-process cancellation. External MCP/socket verification,
installed-service QA and all non-Linux command runners remain unfinished.

MCP now exposes `vm.job_start`, `vm.job_poll` and `vm.job_cancel` for native Linux through
the running app's private agent socket. Start takes a `command` containing `executable`,
literal `args`, explicit `role` (`normal` or `administrator`), `timeoutSeconds` (1–600) and
an optional guest `directory`. It returns a job id for polling; output is bounded byte data.
These tools are experimental and require installed guest tools. They do not invoke the
previous runtime or execute on the host. The socket keeps HPA1 headers bounded to 4 KiB
and carries separately bounded HPG1 bodies (256 KiB requests, 1 MiB replies), decoded in
64 KiB chunks. Bodies use heap storage to keep nested async callers off large stack frames.
The original agent generation is carried from MCP authorization through socket and queue
dispatch. Publication rechecks policy for each output chunk. Peer/directory/socket ownership
checks and connection limits remain shared with native Windows operations.

Three framing/policy tests and two MCP validation/catalogue tests pass locally, as does
strict app/MCP Clippy. The new socket integration tests, existing control suite and existing
MCP machine regression test fail at socket binding with `Operation not permitted` in the
current execution profile. They remain required checks; they were not skipped or reported
as passing. The earlier async stack overflow was fixed and no longer occurs. Actual socket
exchange and native Linux guest start/poll/cancel remain unverified, including service
installation, private boot delivery, process cleanup and runtime/policy races under live load.

A disposable `vzcommands` diagnostic now combines the production daemon, private VZ boot
port and owned queue job API. Its isolated Alpine init checks ordinary-user rejection of
the key file and named device, then the host checks actual UID 1001/0 output and launch
identity, cancelled-job completion, hardware stop and eventual runtime-owner release. It
uses no networking, shared folders or speaker device and never touches VM library records.
This diagnostic does not exercise the external MCP socket or installed Ubuntu systemd unit.

```sh
bash scripts/build/guest.sh
python3 scripts/check/commands.py /verified/initramfs-virt /new/commands.gz native/build/guesttools/hopperguest
cargo build -p machine --example vzcommands --locked
codesign --force --sign - --entitlements assets/hopper.entitlements target/debug/examples/vzcommands
target/debug/examples/vzcommands /verified/Image /new/commands.gz /verified/modloop-virt
```

The current production binary was independently recovered byte for byte from the prepared
initramfs using the system archive reader. The reused Alpine ISO matched its recorded
SHA-256 before modloop extraction. The diagnostic builds and passes strict machine Clippy.
The signed live attempt reports `Virtualization is unavailable on this host`, so its guest
access, jobs, cancellation and cleanup checks have not run successfully. No desktop-readiness
or release claim follows from preparing the diagnostic.

The guest-helper package now carries `licenses.html`, generated from the selected musl-target
normal/build dependency graph and the build toolchain's standard-library copyright document.
Dependency notices retain their actual text, including Unicode's additional terms. The exact
resource manifest hashes the notices, and both bundling and runtime loading require the file.
CI installs the matching `rust-docs` component. The rebuilt helper, notice generation tests,
package validation tests and production seed recovery pass locally, along with engine Clippy.
The declared MIT project license now has its text in `LICENSE`, included in guest notices and
the app resources. This inventory does not close the full redistribution gate: the separately
linked musl runtime and the complete signed app's other bundled components still need an audit.
The release workflow now installs the musl cross target, standard-library documentation and
LLVM/lld/ACPI build tools; its verification job builds and checks the real guest package before
release creation. Bundle verification requires the guest manifest and matching project license.
Workflow YAML, script syntax, notice generation and real guest asset checks pass locally; the
updated GitHub workflow and complete signed bundle have not been run.

Native Linux adds experimental `vm.file_read` through the same authenticated guest job path.
Pass `file` with an absolute guest `path`, byte `offset`, `length` from 1 to 65536 and an
explicit `normal` or `administrator` role. Poll the returned job with `vm.job_poll`; consume
output only after successful completion with exit code zero and no output truncation. Its
stdout is the raw byte array; stderr is a JSON receipt containing the file stamp, offset,
actual length and EOF. Pass the receipt's stamp on subsequent chunks to reject observed
changes or replacement. This is a bounded transfer of files up to 1 TiB, not an atomic snapshot.

The helper runs after the job worker applies and verifies the selected execution identity.
Linux validates a pinned O_PATH inode before opening it for reading, rejects symlink leaves,
nonregular files and proc/sysfs sources, and rechecks inode/size/change metadata around reads.
No host-side reader or legacy VM helper is invoked. Three host reader tests, two shared
contract tests and two MCP catalogue/authorization tests pass; strict host and musl Clippy
also pass. The production guest package rebuilds within its size bound. Actual Linux file
permissions, special-file rejection, CLI receipts and end-to-end guest reads remain
live-unverified. Updated tools must be installed in the guest; large/resumable uploads, directory
browsing, Windows/macOS transfer and drag and drop remain pending.

Native Linux also adds experimental `vm.file_create` for new files up to 32 KiB. Its `file`
object contains an absolute guest `path`, a `bytes` array (0–255) and an explicit execution
`role`. The authenticated typed request delivers bytes through a private sealed memfd to the
guest job's stdin; general commands still receive null stdin. Payload contents do not enter
process arguments. Poll the returned job and require successful untruncated completion with
exit code zero; stdout then contains a JSON file receipt.

The guest helper writes a private mode-0600 anonymous temporary inode under the selected
identity, synchronizes its data and links it into the pinned parent without replacement.
Existing files, directories and symlinks reject without replacement. Destination filesystems
must support O_TMPFILE; unsupported filesystems fail explicitly. Cancellation or failure after
publication can leave the completed destination, so inspect it before retrying. Large/resumable
uploads, replacement and other guest OS implementations remain pending.

Three contract tests, two MCP tests, four transport framing/policy tests, four real package/seed
checks and host/musl Clippy pass locally. Four Linux-only filesystem/CLI tests compile and now
have a dedicated Linux CI job, but have not run. The signed command diagnostic now includes
normal/admin full-payload round trips, existing-file preservation and normal-user rejection of
administrator-owned data. Refreshed diagnostic media includes the current streaming-upload
helper byte for byte.
The fresh signed attempt still exits with `Virtualization is unavailable on this host`; none
of these guest checks passed. The default Docker endpoint is also not connected. Guest job
admission, sealed stdin delivery, atomic publication and cancellation remain live-unverified.

Native Linux now exposes experimental `vm.upload_begin`, `vm.upload_chunk`,
`vm.upload_status` and `vm.upload_commit` through the authenticated guest job transport.
Begin with a `file` containing a new absolute guest path, exact size up to 1 GiB,
SHA-256, explicit role and a 1–600 second timeout. Each reply contains `job`, `offset`,
`size` and `committed`. Send chunks of at most 32 KiB at the current accepted offset;
backpressure may accept part or none. Send only the remainder from the returned offset.
Query status after an ambiguous response. Offsets describe queued bytes, not durable
publication, and do not survive a guest-service reconnect.

After all bytes are accepted, call commit until `committed` is true and then poll the
supervised job. Require successful untruncated completion with exit code zero; stdout
contains an `Uploaded` JSON receipt with the published file stamp and checksum. The
selected-role child receives a private bounded local socket as stdin. Nonblocking sends
carry a checksum header and binary bytes without putting them in argv. The helper requires
all declared bytes, an explicit completion marker, EOF and matching SHA-256 before
anonymous no-replace publication. `vm.job_cancel` closes the producer and cancels the
process tree. Cancellation before commit cannot publish; cancellation or failure after
commit may leave a completed file, so inspect it before retrying.

Upload contract, authentication, transport framing and MCP validation tests pass locally.
Host and Linux-musl strict Clippy pass; Linux-only producer/receiver, backpressure and
cancellation tests compile but have not run. The signed command diagnostic now includes
normal/admin multi-chunk uploads with stamp-checked binary readback, cancellation after all
bytes are queued, rejection of later commit and checksum-mismatch destination absence. Installed guest
service updates, live uploads, full agent-policy lifecycle checks and other guest OS
implementations remain pending. This is not release-readiness evidence.
The refreshed signed diagnostic still exits with `Virtualization is unavailable on this host`
before guest admission; no live upload result is established by that attempt.

Service teardown now records cancellation before closing upload channels, and chunk/commit
operations reject cancellation intent before channel cleanup completes. Four Linux-only
supervisor tests cover uncommitted EOF, teardown, finished jobs and cancellation intent;
they compile under strict musl Clippy but have not executed. The added failure diagnostic
also compiles; its new cancellation/checksum cases have not run.

On 2026-10-12, a fresh unrestricted host run superseded those diagnostic blockers. The
signed `vzcommands` probe passed with the production helper, the official checksum-verified
Alpine 3.24.2 ARM64 kernel/initramfs and its matching modloop, without a network device or
host shares. It verified private boot receipt and ordinary-user key/device rejection,
UID 1001/root execution, 32 KiB file creation/readback and existing-file preservation,
multi-chunk binary uploads and stamp-checked reads, cancellation before commit even after
all bytes were queued, rejected later commit, checksum-mismatch destination absence,
directory pagination, normal-user root-directory denial and proc/sysfs browsing rejection.
The queue also verified cancellation, stopped-hardware rejection and runtime-owner release.

That run exposed and fixed two production bootstrap issues: unnamed console ports must
be skipped during named-port discovery, and auto-bound local sockets may report CID_ANY.
The daemon now verifies the actual guest CID through the Linux local-CID ioctl, retains
host-peer validation and still rejects host endpoints. Failed diagnostic runs preserve
their bounded console evidence instead of deleting the cause of failure.

`vm.file_browse` now accepts a `directory` object containing an absolute guest `path`,
`limit` from 1–32 and explicit `role`, with optional `after` byte cursor and directory
`stamp`. Poll its job for successful untruncated completion; stdout is a JSON listing
with byte-array names, entry kinds, stamp and `next` cursor. Reuse next as after and the
stamp on subsequent pages. Enumeration uses a retained descriptor and sorts by raw name
bytes; symlink entries are not followed. Unknown filesystem entry types remain unknown.
Each page retains at most 33 candidates and rejects scans exceeding 65,536 entries.
Observed directory mutation rejects; this is not an atomic filesystem snapshot.

Three host browse tests, five file-contract tests, two MCP tests and four real package
checks pass. Previously blocked private-socket tests also pass: five input-control tests,
two remote-job tests, nine machine-policy tests and the MCP native-routing regression.
Strict Linux-musl lint passes. Linux-only standalone tests have still not executed, and
the Alpine diagnostic does not prove installed Ubuntu systemd activation, external MCP
round trips into a running VM, live persisted-policy revocation or other OS guest tools.
Those and the full PRODUCT.md release gates remain open.

The signed `cargo build -p mcp --example guestagent` diagnostic now verifies the external
agent route with a real isolated Alpine VM. It runs the production MCP stdio loop in a
separate process, performs initialization, and reaches the owning Host through the
same-user private socket. Normal/root identity jobs, 32 KiB binary create/read, directory
browsing and a multi-chunk upload with checksum receipt and stamp-checked readback pass.
The probe then starts a long-running guest process, persists agent access off, verifies
MCP denial, turns access back on and verifies the old policy generation is rejected,
the process is gone and old session job history is unavailable. This supersedes the
earlier missing external-route/revocation evidence for this isolated native Linux case.

The probe uses an isolated profile and synthetic record with owned Alpine hardware; it
does not install Ubuntu, launch the GUI, exercise the shipping MCP binary's container
engine selection or prove an inside-guest agent connection. The console is bounded to
1 MiB, the child process is killed on diagnostic teardown, failed runs retain evidence,
and cleanup stops and retires owned hardware. Strict MCP all-target Clippy passes.
Installed Ubuntu service activation/update, viewer integration and the remaining
PRODUCT.md release gates are still pending.

Fresh Ubuntu provisioning now uses `systemctl enable --now hopperguest.service` after
installing public guest tools and reloading systemd. Enabling alone left the first boot
without a running service. The isolated `guestagent --ubuntu` diagnostic uses production
media acquisition, unattended preparation, installation observation and system admission
before running the external MCP checks. The initial full acquisition/install run exposed
a closed private boot stream; the subsequent fresh verified-cache installation passes
installed Ubuntu activation and the external checks with the delivery fix described below.

Guest notices now include the complete musl 1.2.5 COPYRIGHT document from its official
release archive, with recorded archive/notice hashes and provenance under
`crates/guest/licenses`. Notice generation requires the reviewed Rust 1.99.0 toolchain
and rejects a missing or altered musl document. CI and release Rust installations are
pinned to 1.99.0 to match this review. Four notice-generator tests, rebuilt helper package
verification and four real package checks pass locally; workflow YAML parses. Other
bundled components and live release automation still require verification.

The verified helper notices now also install inside new Ubuntu guests at
`/usr/share/doc/hopperguest/licenses.html`, owned by root and readable by users. The
bounded NoCloud configuration accommodates the executable and complete notices within
8 MiB. Eleven provisioning/seed/package checks pass, including independent extraction
of the built executable and notices from the production ISO. Strict engine/MCP lint
passes. Read-only inspection after the fresh installed run verifies the executable and notices
match bundled SHA-256, size, root ownership and 0755/0644 modes. The current workspace
and documentation suite passes 1,003 tests with three ignored tests.

The first full automatic Ubuntu acquisition/install run finished deployment and system
admission, but the installed MCP check timed out. Read-only inspection found the helper,
udev rule and enabled unit present; the journal showed `Private boot input ended early`.
Bootstrap now uses a protected duplex console: the guest opens its root-only device and
requests credentials with `HBR1` before the host sends the single-use `HGB1` document.
Delivery stays on the VM queue, checks the original authorization, expires after 180
seconds of running boot and erases its buffered material on completion/failure/teardown.
Five host delivery tests and the real signed Alpine command/file diagnostic pass. A fresh
Ubuntu installation using the production verified-cache path also passes first-boot service
activation and external MCP normal/root identity jobs, binary create/read, directory
browsing, streaming checksum/readback and persisted access revocation/process cleanup.
The diagnostic stops and retires the VM afterward. Read-only post-stop inspection confirms
the service started and the installed executable/notices exactly match bundled checksums
and permissions. Together with the initial official download, this verifies acquisition,
fresh cached installation, system admission and installed guest agent operations. Existing
guest-tool upgrades, GUI/viewer integration and the other PRODUCT.md gates remain open.

The six standalone Linux guest test harnesses also pass inside an isolated signed Alpine
VM as root and UID 1001: 48 passed, none failed. This executes the previously compile-only binary-name, file
read/create, sealed-input CLI, streaming receiver/CLI, producer backpressure/cancellation
and bootstrap tests. `scripts/check/guesttests.py` packages bounded stripped cross-compiled
harnesses from Cargo JSON artifact output for the `vzguesttests` example. The test-only
`HOPPER_GUEST_TEST_EXECUTABLE` override points CLI tests at the packaged public helper;
it does not change production guest commands. This closes the earlier missing Linux
standalone-test evidence for these harnesses, not the broader desktop/release gates.

Native Windows snapshot work now includes bounded serialization of Apple's interrupt
controller state. A signed `interruptstate` diagnostic restores pending wired and MSI
interrupts into a freshly recreated VM and checks invalid-state rejection. Running
snapshots still need the remaining device and disk state, PMU preservation, plus
recoverable publication and live open-application restoration. No native snapshot
capability is enabled by this component alone.

The CPU-state foundation now preserves general and FP/SIMD registers, core system
registers, CPU interrupt-interface state, timer offset/mask and pending SError on the
CPU owner. A signed `cpustate` probe recreates the VM, verifies state equality and
continues real guest instructions with nonzero thread, stack, FP and SIMD values.
The standalone architectural record excludes PMU state; coordinated runtime capture
preserves the exposed cycle-only PMU separately. Wider vectors and complete durable VM
serialization remain unfinished.

Architectural CPU state now has a bounded HCS1 binary record. Its little-endian bank
counts and explicit register identities preserve general, FP/SIMD, system, debug and
CPU interrupt-interface values plus timer offset/mask and pending SError. A SHA-256
checksum covers the header and payload; records are at most 4 KiB. Unknown identities,
dimensions/flags, unsupported SVE/SME features, extra/truncated data and damage reject
before hardware allocation. Owned encoded/temporary register buffers erase on drop.
The checksum detects damage; snapshot authorization still needs a VM-bound manifest.
Five codec checks cover full/maximal banks, every truncation and single-bit corruption,
rechecksummed foreign schemas, wider vectors and incomplete state.

The signed CPU diagnostic can save to a private file and restore in a separate process.
It syncs the mode-0600 file and its mode-0700 directory, destroys the original hardware
and state, then reopens bounded bytes and continues guest instructions in fresh hardware
with the saved nonzero register/vector/debug values. The repeatable check also verifies
symlink, hardlink, FIFO, directory, public/oversized file, public parent and checksum
rejection, and preservation on an attempted overwrite:

```sh
python3 scripts/check/cpustate.py
```

This standalone record covers architectural CPU state only. The coordinated HCP1 record
now bundles it with an HPM1 cycle-only PMU record and the frozen guest counter, within
8 KiB. Explicit PMU register identities, CPU affinity/features and counter frequency bind
the components together. The normalized host timebase must match at decode, and the
guest counter frequency must agree with those host units. Checksums detect corruption;
they do not authorize a snapshot or bind it to a VM. Unsupported event-counter banks,
mixed components, changed timer units and malformed/damaged records reject.

The signed `pmurecovery` check syncs private records for both CPUs, destroys the original
CPU/PMU/clock records, reopens them, and reconstructs hardware twice. Both enabled timer
types retain their delay across the initial hold, and distinct PMU settings/cycle epochs
survive. A separate signed check rejects a foreign CPU affinity without changing the
destination architectural context. Four combined codec checks cover round trips,
truncation/damage, rechecksummed foreign metadata and mixed-component encoding.
Remaining device state and disks remain retained in process during this diagnostic. Their
durable serialization, bound manifests, atomic publication/recovery and complete durable
VM snapshots remain unfinished. No worker or agent snapshot endpoint is enabled.

Interrupt-controller state now has a bounded HGI1 record containing Apple's opaque
SDK-versioned bytes, explicit payload length and SHA-256 checksum. Payloads are limited
to 16 MiB; owned state and encoded buffers erase on drop. The record decoder checks
framing/integrity; the SDK still checks opaque-state compatibility at restore. A checksum
does not authenticate the state or establish matching VM topology.

The signed GIC check saves a private record and exits, then restores it in a fresh process.
Enabled peripheral/MSI interrupts, their pending bits and priorities survive; the restored
controller accepts MSI clear and re-signal. The check also rejects symlinks, hardlinks,
FIFOs, directories, public/oversized files, a public parent, damage and overwrite attempts
without changing the original record. Two codec tests cover payload bounds, round trips,
truncation/damage, extensions and rechecksummed length mismatches. The two-CPU
`pmurecovery` diagnostic now reopens GIC records alongside CPU/PMU/clock records before
repeated reconstruction. This does not establish full VM durability or guest interrupt
handler execution after process reconstruction.

```sh
python3 scripts/check/gicstate.py
```

HRM1 now streams firmware, NVRAM and RAM captures in 64 KiB chunks, omitting all-zero
chunks. The header binds guest address, logical size and chunk size; a SHA-256 checksum
covers the header and stored chunk framing/data. Logical banks are bounded to 64 GiB,
must be host-page aligned, and must exactly match the caller's authorized address/size
before reopening allocates memory. Decoding uses private anonymous memory and seals its
maximum protection read-only before returning. The writer and reader recheck authorization
between chunks and before success. Blocking file I/O itself still needs caller ownership
and bounds. Failed writes must remain unpublished even when all bytes reached storage.
The checksum detects damage; it does not authenticate a snapshot.

The signed memory check saves/exits and restores in a separate process. Repeated fresh
VMs execute reads and writes from reopened memory; zero/nonzero regions and the final
chunk survive, while guest/host writes leave the saved mapping unchanged. Its sparse
32 MiB fixture occupies under 256 KiB. Unsafe-file/checksum/overwrite checks also pass.
Three codec checks cover sparse/partial chunks, immutable protection, trusted-bank
mismatches, malformed dimensions, truncated/damaged/trailing input, cancellation and
failed writes. The two-CPU recovery diagnostic now syncs all three memory banks, destroys
their original mappings, reopens the records and continues both guest CPUs twice with
the saved timers and PMU settings. Remaining device state and disks remain in process.
Large populated-bank throughput, maximum-size memory pressure, durable device/
disk records, authenticated manifests, atomic publication/recovery and complete VM
process restoration remain unfinished.

```sh
python3 scripts/check/ramstate.py
```

HSW1 now records native software CPU state and local boot actions within 512 bytes:
one or two unique CPU affinities, their online/parked state and debug locks, up to sixteen
ordered executable regions, a UART tail of at most 64 bytes and an optional relative
boot-key delay of at most one second. Strict counts, booleans, primary-online status,
topology and action bounds reject malformed state. Restore still checks the authorized
runtime topology before changing software state. HUR1 separately preserves PL011 control,
line and baud registers and up to sixteen pending receive bytes within 69 bytes. Queued
input survives a FIFO-mode change even when the new receive capacity is smaller.
Both records use SHA-256 for corruption detection, without snapshot authentication.

The signed recovery checks sync/reopen these records after destroying their original
objects. A parked secondary stays off until guest CPU_ON starts it; a pending boot key
retains its relative delay through a 1.2-second hold and delivers once after Continue.
Guest-written UART registers survive reopening and the console can transmit afterward.
Five codec tests cover these records, damage/truncation, rechecksummed malformed values,
unsupported topologies and rejection of foreign restore topology without mutation.
Remaining devices and disks still remain in process. Full durable
runtime reconstruction, atomic publication/recovery and agent snapshot endpoints remain
unfinished.

HFL1 now records flash geometry, block protection, mode/status and every pending command
within 263,000 bytes. Buffered writes preserve their base, remaining word count and up to
32 ordered address/value pairs. Decoding checks geometry, exact protection count, modes,
status, command fields and bounded aligned addresses; duplicate or cross-window buffered
writes reject. NVRAM contents remain in their separately recorded memory bank. Encoded
buffers erase on drop, and SHA-256 detects corruption without authenticating the snapshot.

The command-boundary tests now destroy the original captured command object and reopen
its record before continuing each command trace. Additional checks cover the largest
protection inventory, a full buffered write, damaged/truncated/extended records and
rechecksummed unsafe fields. Signed reconstruction syncs/reopens flash alongside the
other records. A guest paused midway through a buffered flash write resumes in fresh
hardware and completes the write/confirmation; the saved NVRAM bytes stay unchanged.
Snapshot manifests, atomic publication/recovery and full VM restoration
across app restart remain unfinished.

HVT1 now records Virtio transport metadata within 8 KiB: stable backend-kind tags,
offered/driver features, common registers, queue registers and quiescent ring positions,
ISR, PCI window state, completion count, bounded fault text and MSI-X state. Nested HPC1
PCI records bind configuration and BAR geometry to a caller-supplied matching function
layout, preserving BAR addresses/probe halves while rejecting immutable or reserved
configuration changes. HQR1 queue records check dimensions, alignment, overflow, region
overlap and initialization/progress consistency; restore additionally checks matching
guest RAM before mutation. HMX1 preserves up to 64 MSI-X entries and pending bits, with
reserved/control-bit checks. Every format has a SHA-256 corruption checksum.

The signed group diagnostic now syncs/reopens transport records alongside the other
components before full runtime reconstruction, recapture and guest execution. Queue tests
check index wraparound without replay and preserve deferred used-ring initialization.
PCI/MSI-X checks use fresh layouts, verify BAR probes and masked pending-message delivery
exactly once, and reject damage, truncation and rechecksummed invalid state. Transport
reopening still requires a matching backend/layout binding. Full snapshot process
restoration, publication/recovery and agent endpoints remain unfinished.

HIN1 now records input kind, capability selectors, LEDs, up to 1024 pending events and
delivered held keys/buttons within 9 KiB. Tablet axes retain their already-normalized
values. HVC1 records the named guest serial port, negotiation flags, bounded control
messages and both 64 KiB byte buffers within 134,000 bytes. Both codecs validate their
schema/state bounds and carry corruption checksums. Backend reopening destroys the old
contents before decoding, retaining only the expected input kind or serial name; a
foreign binding rejects. Host input ownership and authenticated job sessions are excluded.

Pure checks verify release of delivered keys without sending queued presses, LED/selectors,
tablet coordinates, maximum input queues and full console buffers, negotiation controls,
damage/truncation, malformed state and foreign bindings. Signed group recovery now
syncs/reopens input backend records before device use, recapture and delayed-key delivery.
The console record is verified through simulated guest queues; installed guest serial
exchange after reopening remains unverified. Atomic publication/
recovery, full process restoration and agent snapshot endpoints remain unfinished.

HGF1 now streams GPU resource pixels/backing ranges, scanout, published frames, cursor
and linear framebuffer state with a corruption checksum. Resource pixels and published
pixels remain independent, preserving drawing not yet flushed to the screen. Active
linear layout is independent of pending registers. Reads/writes use chunks of at most
64 KiB; display binding, dimensions, resource allocation and span counts are bounded
before allocation. Restore additionally checks backing ranges against restored RAM.
The maximum record is bounded below 400 MiB; production publication remains unfinished.

GPU checks reopen records before testing unflushed updates, cursor pixels, active linear
layout, repeated independent recovery, malformed state, revocation and failed writes.
Signed group recovery syncs/reopens GPU files after destroying original GPU state and
verifies the linear frame before/after resumed execution and recapture. One group attempt
failed at a PMU checkpoint; a subsequent diagnostic run and standalone two-CPU PMU
recovery passed. The PMU failure remains
unexplained; the added error records the register, expected checkpoint and observed PC.
This does not establish complete snapshots across app restart or open-app restoration.

HDS1 now records image capacity and a full logical-byte SHA-256 checksum. Saving clones
the captured image into a caller-owned private staging directory, syncs it and refuses
replacement. Reopening uses pinned directory descriptors, rejects unsafe image files,
clones into fresh private ownership and verifies the clone before returning it. HDB1
binds that image receipt to disk identity, capacity, read-only mode and writeback state.
Both receipts carry corruption checksums; snapshot authentication and publication are
separate. Hashing includes sparse holes and runs outside the capture cut; it can be
expensive for large images, and authorization checks do not bound OS syscall duration.

Separate-process disk checks save a guest-written sector, terminate its original owner,
then recover/read/write it in two fresh processes without changing the saved sector.
Storage checks cover private modes, symlinks, hard links, damage, capacity/binding
mismatch, path replacement, cancellation and independent sparse APFS forks. Signed
group recovery now syncs/reopens disk records before fresh runtime execution and
recapture. Manifests, crash recovery/garbage collection, full VM process
restoration and worker/agent snapshot endpoints remain unfinished.

HDO1 now preserves optical-media state in a fixed 2,270-byte checksummed record with an
HDS1 image receipt. It includes negotiation, insertion/ejection, pending sense, all command
and queue counters, and bounded diagnostic LUN/control/transfer fields. Reopening checks
capacity binding and canonical state before media access, drops original captured media,
then verifies an independent APFS image copy. Existing installer paths are not reopened.

Separate-process checks terminate the saver, recover in two fresh processes, verify
ejection and pending sense, then reinsert and read captured sectors despite different
target contents. Other checks cover all retained SCSI statistics, minimum/maximum request
sizes, read-only rejection, malformed state and final authorization. Signed group recovery
now syncs/reopens optical records before execution and recapture. Component persistence
does not yet supply an atomic VM manifest, crash recovery, process restoration, rollback,
open-app recovery or agent snapshot endpoints.

Device records can now decode against fresh owned hardware without retaining original
captured transport or backend objects. HVT1 binds to that hardware's PCI geometry,
features, queue count and MSI-X layout. Separate backend records bind to its input kind,
serial name, preferred display dimensions or disk/media capacity and identity. Payload
type mismatches and foreign bindings reject; authorization is checked before parsing,
between components and before returning the complete state. Restore still validates
matching guest RAM before applying queue progress or attaching recovered backends.

The signed group diagnostic now destroys every original checkpoint object before it
constructs fresh expected devices and decodes records. Input, serial, graphics, disk and
optical tests exercise this path. A separate-process input check recovers pending events,
held-key releases, BARs, queue progress and interrupt status after the saver exits, twice.
This removes an in-process dependency; complete VM process restore still requires
production publication, crash recovery, rollback and worker/agent integration.

HSM2 now binds a complete component inventory to caller-supplied machine/snapshot IDs
and a configuration fingerprint. It records CPU count, three memory banks, seven device
slots and up to 24 component commitments in at most 1,200 bytes. Missing, duplicate,
unordered, foreign or oversized entries reject. Each commitment contains length and
the component's existing SHA-256 payload footer, avoiding a second hash of large memory
records. Every component decoder must still verify its complete payload.

A bounded reader checks the length and footer of the bytes actually consumed, including
fragmented reads, and checks authorization around reads and final admission. It rejects
swapping a valid same-length record from another snapshot. Matching a footer alone does
not establish payload integrity; decode and manifest admission must both succeed before
the checkpoint is usable. Disk/media receipts separately bind captured image contents.

The signed diagnostic now syncs/reopens a manifest with explicitly synthetic identity,
then verifies all consumed components against fresh hardware before execution/recapture.
One attempt failed before manifest loading when a secondary CPU stopped unexpectedly;
the rerun passed. Finished-worker errors now expose their underlying cause, but that
intermittent failure remains unexplained. Production identity/configuration binding,
worker integration of package publication, crash recovery, full VM process restore,
rollback/open-app recovery and worker/agent snapshot endpoints remain unfinished.

PMU accesses now restart their fixed instruction slot and original write operand after
a canceled exit, with the existing 16-attempt and context deadline bounds. Fault injection
at a completed HVC checkpoint reproduced entry into the adjacent access slot before the
fix; the regression now checks interrupted reads/writes, operand restoration and bounded
rejection without changing the original CPU context. A live interruption diagnostic
also completes 1,024 captures while another thread requests CPU exits, preserving PMU
controls and architectural state. Six signed group runs passed before this change; the
natural intermittent failure did not reproduce. The injected retry defect is fixed,
but its relationship to the earlier PMU/secondary-owner failures remains unproven.
Three subsequent signed group runs and the standalone two-CPU PMU recovery diagnostic
passed, along with the full machine tests and strict all-target lint.

Snapshot packages now stage under a pinned private parent directory and publish with
an exclusive atomic rename. Typed component writers bound each write to 64 KiB and the
record limit, verify the complete payload checksum while writing, sync and seal files
read-only, and reject failed encoders even after a complete footer. Publication requires
the full ordered inventory and syncs its manifest, stage and parent. Existing snapshots
are never replaced. Errors after rename can leave a complete package; callers must
inspect that identity instead of overwriting it. Abandoned stages remain preserved;
crash recovery and garbage collection are not implemented.

Package tests verify reopening after the writer process exits, no replacement, every
write/publication authorization checkpoint, incomplete/corrupt writes, fragmented large
writes, unsafe file rejection and pinned-parent replacement isolation. The signed group
recovery diagnostic now uses the publisher for real CPU, RAM, graphics, input, disks,
optical media, flash and software records, then reopens against fresh hardware after
original state teardown. Component decoders still validate schemas and image receipts;
a package alone does not establish a usable VM. Diagnostic identity remains synthetic.
The machine library now owns complete checkpoint saving/loading. Saving streams the
immutable captured cut into its package after the brief VM pause; image encoders retain
the original authorization during full-image hashing. Loading checks independently
configured device kinds, decodes every component and verifies consumed commitments before
returning any checkpoint. It does not allocate or start guest hardware.

`scripts/check/vmstate.py` signs an isolated diagnostic executable, saves a complete
running two-CPU VM checkpoint, waits for the source process to exit, then restores from
two independent new processes. Both resume saved CPU loops/UART traps, recover graphics,
keyboard and disk/media state, and recapture the running VM. Package hashes, inode/size,
mode/link count and modification times stay unchanged. The check rejects duplicate
publication, valid same-length CPU substitution, checksum damage and revocation at
initial/open/load and final admission checkpoints. Full machine tests and strict
all-target lint pass. This uses a minimal native test guest and synthetic binding;
installed-desktop/open-app recovery is not established. Production identity/configuration
binding, worker/agent integration, crash recovery/GC, rollback and snapshot/clone endpoints
remain unfinished.

Installed Windows now has a signed fresh-process package recovery diagnostic too.
`scripts/check/windowsstate.py` APFS-clones a private disposable fixture, captures its
running 4 GiB/two-CPU guest, publishes a complete package, waits for source-process exit
and loads it through the machine library in a new process. The captured/held frame matches
exactly. Visual inspection verifies the Windows lock screen before capture and sign-in
after guest Space input following recovery; disk requests continue and no device faults
are reported. Package inode/size/mtime and source fixture metadata remain unchanged.
A clean clone of the original installed source was used: the first, repeatedly stopped
fixture had reached Windows recovery instead, and its serialization was canceled with
its unpublished stage preserved. That attempt is not desktop recovery evidence.

The clean run serialized the package in 52 seconds, decoded it in 43 seconds, rebuilt
held hardware in 46 ms and ran recovered Windows for 30 seconds. Serialization/hashing
happens after the coordinated capture and source teardown in this diagnostic. macOS ARM
builds now enable the pinned SHA-256 library's detected hardware path, with software
fallback; standard empty/abc/million-a vectors, fragmented updates, machine tests and
strict all-target lint pass. Its additional MIT notice ships in app resources and is
checked by bundle verification; the Linux guest dependency graph remains unchanged.
Full signed bundle verification and the broader redistribution audit remain pending.
Installed Windows open applications, Windows-specific PMU semantics and production snapshot/rollback/clone integration are still
unverified or unfinished. Linux/macOS installed-desktop snapshot recovery is also pending.

Windows clock initialization is now corrected in the native runtime. Its existing
firmware clock driver had no clock device or device-tree node to discover. Hopper now
supplies a PL031 clock at a dedicated MMIO address, seeded from host UTC, with byte
identity reads, 32-bit register access, guest-only time adjustment and bounded alarm
state. Guest writes never change host time. The wall clock follows host UTC while CPUs
are held; an explicitly disabled guest counter remains stopped.

Native startup now carries the saved VM's canonical `machineId`. The session checks it
against the VM it owns before launching, and the worker must echo the same identity when
hardware is allocated. Missing, malformed or foreign identities reject; failed startup
reaps the worker. Parent and worker must be packaged from the same build. A signed
synthetic two-CPU worker check exercises identity, status, pause/resume and stop. This
binds startup to the requested record; snapshot configuration/package integration and
open-application recovery remain unfinished.

Startup now also returns a hardware fingerprint frozen before guest execution. It binds
VM identity, CPU/RAM, firmware and device geometry/identity, while allowing captured guest
contents and runtime settings to change. The parent retains it with that worker session;
missing or zero values reject startup. Fresh-process checkpoint diagnostics use the same
fingerprint helper instead of a fixed configuration value. Signed worker checks verify
stable values across independent starts and different values after CPU or firmware changes.
Component and image checks remain required separately. Production snapshot endpoints,
rollback, clones and open-application recovery remain unfinished.

The persistent worker runtime now retains normal firmware durability while supporting
coordinated capture ownership. Signed diagnostics capture a pending firmware write,
resume and commit it in the source, reopen the durable store, then continue the captured
state independently without changing that source. Two-CPU checkpoint creation also uses
this persistent runtime. The recovery runner includes these signed checks before its
fresh-process package tests. Persistent restore publication and worker/agent snapshot
commands remain unfinished.

Checkpoint publication now has an owned background writer with cancellation, deadlines
and original-policy checks through completion delivery. Teardown joins the writer before
releasing its supplied ownership token; filesystem calls can delay cleanup. A cancelled
operation can leave a complete package after atomic publication, which must be inspected
rather than replaced.

Publication also requires an explicit commit guard. It rechecks the original authorization
under that guard before rename and retains it through sync and the final check. Filesystem
lock tests cover contention from another process, policy changes before guard acquisition,
failed acquisition and error cleanup. Production policy-lock wiring remains unfinished;
isolated diagnostics use a no-op guard for their synthetic policies.

The host policy token now checks the saved original agent generation and VM identity,
rejects bounded/no-follow record failures permanently, and acquires the same access lock
used by policy changes. Tests verify the real access setter across processes and package
publication under this guard. Transporting that guard to the worker remains unfinished;
it does not supply the VM operation/runtime lease.

Complete checkpoint serialization also supports a sealed-stage handoff. A fixed-field
receipt binds its directory inode and manifest; a host reopens it within a pinned private
namespace and commits under the original saved-policy guard. Tests verify writer-process
exit before host publication, receipt/manifest and directory replacement rejection, duplicate
commit and cancellation. Signed two-CPU checks exercise actual checkpoint staging and
reopening after capture teardown. Worker protocol handoff and its runtime/operation ownership
remain unfinished; a receipt alone authorizes nothing. Full restore still validates component
schemas, payload checksums and disk-image receipts.

The background writer can now return a sealed stage without publishing it. Cancellation,
deadline and original-policy checks survive completion collection, and its supplied owner
remains retained through the caller's commit. Tests cover cancellation after stage collection
and revoked cached results. The signed save diagnostic collects and reopens a staged receipt
before committing under its original job checks; this commit runs on the diagnostic owner
with a synthetic-policy guard. The private worker protocol is now present below; dedicated
host capture operation management is described below.

Signed diagnostics modify source devices after capture, resume both CPUs before serialization
begins, continue CPU/UART execution, then restore the saved package in fresh processes.
Revocation before or during image verification prevents complete publication. Lifecycle
tests also cover cached-result revocation, cancellation, expiry and worker panic. This
background writer is not yet connected to production worker/agent snapshot commands;
operation exclusion, crash recovery, persistent restore, rollback and clones remain open.

Private worker commands now capture and stage snapshots, poll their fixed-field receipts,
and cancel retained work. Running sources resume after capture; manually paused sources stay
paused. One retained job and bounded renewal/deadlines limit private staging. These commands
accept no host paths and cannot publish; their parent-supplied generation tag and renewal
lease do not replace saved agent authorization. Public session requests refuse them until the
dedicated host capture controller below supplies the trusted route.

Signed checks exercise the actual worker, including namespace symlink rejection, receipt
binding, pause preservation, lease expiry and stop cleanup. A separate host process commits
its stage under the saved-policy guard while the worker remains alive, decodes every record,
and rejects another stage after policy off/on. These isolated checks do not establish installed
Windows application recovery. The host controller below now handles runtime/operation
ownership and cancellation; snapshot indexing/UI/MCP routing and persistent restore/rollback
remain unfinished.

The native host capture controller now freezes saved policy before waiting, retains its
session and operation owners through cleanup, and rechecks the original runtime generation,
cancellation and deadline during commit. Agent connections can supply their upstream original
policy generation. It uses startup-pinned namespace/topology capabilities and rejects replaced
VM directories before dispatch. Caller cancellation leaves a cleanup owner running; uncertain
cleanup stops/reaps the original worker before releasing ownership. If process cleanup cannot
be verified, the operation remains owned and reports an error pending recovery. Cancellation
after publication preserves the completed package.

Eight controller tests and the session/policy/wire regressions pass, along with strict lint
and app test compilation. A signed real worker profile verifies complete snapshot decoding,
duplicate preservation, revoked-agent rejection and manually paused capture. Snapshot names/
indexing, UI/MCP routing, persistent restore/rollback, clones and installed application recovery
remain unfinished.

Native saved-package discovery now reads committed manifests from a pinned private
namespace. It bounds directory enumeration and manifest reads, preserves unknown entries and
unfinished stages, checks authorization throughout, and rejects foreign, corrupt, symlinked or
nonprivate published packages. Discovery checks inventory; restore must independently validate
hardware and decode every component. Snapshot names, production list routing, recovery and
cleanup remain unfinished.

Native Windows `vm.snapshots` now lists committed captures without a running app or worker.
The saved-policy catalog opens the identity-derived private namespace, rejects symlinks and
replaced directories, and retains the request's original agent generation. Unprepared VMs
return an empty list without allocating state. Entries include snapshot/VM ids, saved hardware
fingerprint, CPUs and RAM; listing does not establish restore compatibility. Host/MCP catalog
checks and the signed post-worker-teardown check pass. UI listing, MCP restore and persistent rollback remain unfinished.

Named native capture stores its name and host creation time inside the same atomic package
publication. A bounded checksummed description binds them to the exact manifest and VM;
corrupt descriptions reject listing. Older unnamed captures remain listable. `vm.snapshots`
includes optional name/time fields. Controller cancellation after publication preserves these
fields, and the signed worker check verifies them after teardown. UI named capture, name editing and restore/rollback remain unfinished.

Native Windows `vm.snapshot` now creates named captures through the running Hopper app.
The private socket carries identities/name and the original access generation, then dispatches
to the owned capture controller. Disconnect and pipelining cancel the request while cleanup
retains VM ownership. Successful replies follow worker cleanup; a lost reply may leave a
complete capture, so inspect `vm.snapshots` before retrying. Socket/MCP checks and a real signed
socket capture pass. This remains experimental: native restore, installed application recovery,
UI snapshot controls and the remaining release gates are unfinished.

Persistent native restore can now prepare a separate EFI store from admitted captured NVRAM
and flash command state. It rejects existing stores, checks original authorization during
bounded reads and preserves a newly written namespace after failure. Restored hardware rejects
mismatched durable bytes before allocation, then journals confirmed guest NOR changes. The
signed pending-write check verifies restored durability without changing the source store or
capture. Unit tests and strict lint pass. Host restore publication, disk/firmware rollback,
crash recovery and user/agent restore remain unfinished.

Captured disk state can now prepare a persistent sparse writable APFS clone in a pinned
private restore stage. Names are exclusive, cancellation preserves written data and the
returned owner retains an explicit disk lock. Tests cover independence, namespace replacement
and lock release with duplicated descriptors. Signed recovery checks exercise preparation from
a captured PCI disk and reopening its writes while the saved disk remains unchanged. Atomic disk/EFI publication is still unfinished, along with rollback and restore controls.

Persistent runtime recovery now requires prepared files for every captured disk and adopts
their locked descriptors rather than silently creating ephemeral forks. It validates source
identity, complete contents, private namespace and controller binding before guest execution.
Original authorization applies during admission and around owner callbacks; loss of access
before Continue stops reconstruction. Returned stopped devices retain the disk lock until drop.
This remains trusted core integration. Host publication/selection, rollback, crash recovery and
user/agent restore are unfinished; full-disk comparison currently adds two logical disk scans.

Signed verification now covers capturing again after persistent restore: both restored CPU
loops advance, disk writes persist, a fresh complete checkpoint is sealed/reopened after
teardown, and two independent persistent recoveries retain their own writable disk state and
locks. The original checkpoint remains unchanged. Source and two fresh reader processes pass
the full recovery pipeline; strict lint passes. Installed application recovery and host/user
restore integration remain unverified or unfinished.

On Apple silicon, EFI stores now retain a private directory handle. Bank reads, compaction,
journal opens and relative restore creation use that handle, so renaming a restore stage cannot
redirect later persistence through its old path. Tests cover parent and child renames, replaced
paths, exclusive creation, symlinks, hard links and private permissions. The signed persistent
recovery diagnostic prepares disk/EFI under a stage and renames it before restored execution.
The bound-generation layer below builds on these pinned handles. Atomic host selection,
rollback and crash recovery remain open release gates.

Native recovery generations now stage all captured writable disks and EFI together. A bounded
HGR1 receipt binds the generation identity to the complete admitted checkpoint manifest.
Preparation decodes every checkpoint component against independently authorized hardware;
publication retains a caller-supplied commit guard, rechecks pinned stage/child identities and
uses exclusive rename. Reopening requires the original package and matching hardware, verifies
all disk bytes, and retains disk/EFI locks through handoff. Failure preserves staged data;
cancellation after rename can leave a complete generation. This is trusted core integration,
not host restore selection. Publication, reopening and handoff currently add full logical-disk
comparisons; filesystem calls have no hard deadline. The consumption guard below prevents
replay after execution handoff, even when disk/EFI bytes have not changed. Host selection,
rollback, crash recovery, UI/MCP restore and installed-application verification remain unfinished.

The signed generation diagnostic passes source capture and two fresh-process recoveries,
including publication, teardown/reopen, persistent execution and independent recapture. It
rejects altered receipts, replaced stages/EFI children, revoked preparation and denied commit
guards; cancellation after rename preserves a reopenable generation and duplicate publication
never replaces it. The runner requires three successful generation handoffs and the publication
failure proof in each reader. Targeted storage/EFI tests, strict machine lint and Python
compilation pass. These checks use synthetic guests; they do not establish installed-app restore.

Generation handoff now uses `consume`, which retains the original policy commit guard while
exclusively writing and syncing a fixed HGA1 attempt marker. The marker binds the HGR1 receipt
and a fresh nonce. No executable resources are returned until file and directory durability,
original authorization and pinned child identities are rechecked. A denied guard leaves no
marker; a failure after marker creation preserves it and releases disk/EFI ownership. Any
existing marker, including partial files, symlinks, directories or FIFOs, rejects reopening
without reading or deleting the entry. This conservative guard also blocks replay when a
handoff fails before guest execution. The host still needs prior-state preservation, active
selection, rollback and crash-recovery controls; consumption alone does not implement them.

The signed attempt checks pass in the capture process and both fresh readers. They verify
marker identity/checksum, cancellation after its durable write, lock release, guard denial
without a marker and rejection of partial/symlink/directory/FIFO entries. Each reader also
rejects replay after complete runtime/device/EFI teardown in three independent recoveries.
The existing recovery, recapture, authorization, corruption and source-immutability pipeline
passes, along with sixteen targeted storage/EFI tests, strict lint and Python compilation.
These are synthetic runtime checks; actual process-crash recovery and host transaction/UI
integration remain unfinished.

The host now persists initial restore intent in a private, checksummed 149-byte HRR1 record.
It binds the VM/configuration, selected generation, source checkpoint, rollback checkpoint and
original predecessor, with Prepared/Attempted/Active phases. Selection checks the published HGR1
against its exact source manifest, requires the prior capture manifest and rejects any existing
attempt marker. A restore owner retains VM operation/runtime exclusion and the original saved
policy; commit uses the actual access-mutation lock. Attempt publication compares the original
record and atomically replaces it. Failures preserve committed or partial recovery state.
Normal system/installer preparation rejects pending, damaged, foreign or unsafe restore records
before accessing the original disk; it never silently falls back while intent exists.

Forty-seven primary host tests pass, including writer exits without destructors, cross-process
exclusion, cancellation after commit, policy revocation and a previously bootable original disk.
Strict engine/machine lint and the signed snapshot-controller diagnostic pass. The latter uses
real source/prior captures and a prepared EFI generation, persists intent before core consumption,
reopens intent after owner teardown and checks startup/replay rejection. Its synthetic guest has
no system disk. Installed-guest recovery, active-generation cold startup, prior-state rollback
and UI/MCP restore remain unfinished; held worker and registry admission are described below.

The private worker protocol now supports held restored admission. StartRestored binds canonical
checkpoint/generation identities and the original policy tag; its Restored reply echoes those
bindings and the independently reconstructed hardware fingerprint. The worker opens the source
package and prepared generation relative to a pinned original VM namespace, consumes the durable
attempt marker, adopts persistent resources and reconstructs hardware while paused. A matching
RestoreResume is required before execution; ordinary, stale and replayed resume requests reject.
The pending hold has a 30-second deadline. Snapshots continue to use the VM's original snapshot
namespace while EFI writes use the generation-owned store. Public session requests cannot
forward either restore command.

Trusted host launch/resume helpers own the original restore owner and require an Attempted HRR1
selection. Startup runs in a retained background task and polls original authorization and caller
cancellation during reconstruction; it does not hold the access commit guard during long disk
validation. The already-committed host intent and conservative worker attempt marker survive
interruption. Explicit resume retains the actual policy commit guard through its reply. Invalid
post-start authorization/configuration stops and reaps the worker before returning failure.
Cancelling either caller leaves the owning task to drain and clean up. The worker monitor retains
runtime/operation/journal ownership through actual child reaping, including after the last client
drops; unconfirmed reaping quarantines ownership. Hardware preparation does not acquire or rewrite
the original EFI bank.

Five process-level ownership tests cover startup cancellation, revocation during reconstruction,
configuration rejection, cancelled resume and retention past the stop acknowledgement until exit.
The wider native/model/restore/wire/startup/session/controller suite and strict lint pass. The
signed controller verifies held startup, denied ordinary/stale/replayed resume, explicit resume,
stop/reap, original-bank preservation and exclusive ownership after the caller drops its owner,
with release only after process reaping. Its synthetic guest has no disk. The prepared handoff now connects to the native registry as described below. Active-generation
cold startup, rollback, installed-application recovery and UI/MCP restore remain unfinished. Full disk-validation performance, crash recovery and unconfirmed-reap
recovery remain unverified.

Trusted Sessions::start_restored consumes the prepared Owner and publishes a restored client into
the ordinary native session slot after held admission and explicit authorized resume. It binds the
owner to the registry, saved VM identity and original directory inode, rechecking before publication.
Publication retains the actual policy commit guard. Only then does it release the preparation
operation lease so guest/viewer operations can take their own leases; runtime and journal ownership
stay with the worker until reaping. Session stop waits for that ownership release. Existing state
watches follow the new worker generation. Caller cancellation before publication leaves the helper
tasks to drain and stop the unpublished worker, and registry teardown cannot be prevented by a
surviving viewer watch. Six process-level tests cover ordinary operations, revocation, shutdown,
teardown, startup/resume cancellation and foreign/replaced namespaces. This trusted prepared
handoff is not an app restore flow or agent restore endpoint. The signed two-profile diagnostic
verifies both direct held admission and registry status/pause/resume, viewer-watch state, stop/reap
and original-EFI preservation using real native workers. Sixty-four primary native/model tests and
strict lint pass; the signed guest has no disk and does not prove installed-application recovery.
Automatic preparation/prior-state
capture, active-generation cold startup and installed desktop/viewer recovery remain pending.

Restored resume now atomically advances HRR1 from Attempted to Active, seals and syncs the new
record and syncs its containing directory before dispatching RestoreResume. The original access
commit guard remains held through that command's reply. Both host and worker use the shared
machine::snapshot::selection codec. The worker requires matching Attempted selection at admission
and independently reads an exactly matching Active record before allowing its held CPUs to run.
The full VM/generation/source/configuration/rollback/predecessor binding is compared, not just its
phase. Original boot remains blocked for every phase; Active is durable recovery intent.
The trusted cold-boot route described below uses the current generation without replaying the checkpoint. Errors after activation preserve it and stop/reap the worker.

Restored startup binds its boot store to the owner's original VM namespace and polls inode
identity during reconstruction; activation rechecks that binding before and after commitment.
Process tests verify activation is visible before resume acknowledgement, policy mutation remains
excluded until its reply, cancellation preserves Active and replacing the namespace prevents
activation. Reopening an Active generation refuses a second checkpoint startup without
creating a worker. A synthetic-worker child exits without running destructors; its Active selection,
prior/source generations and original-boot exclusion survive, and the orphan observes pipe EOF.
This proves host journal/process behavior, not installed guest crash consistency. Full active
system startup/reboots and application recovery remain unfinished. Sixty-nine primary
native/model/restore tests, child-process probes and strict lint pass. The signed direct and
registry probes verify activation-gated execution and retained Active intent after teardown.

The core generation::System admission path opens current mutable disks and firmware for fresh
system boot, without decoding captured CPUs, RAM or device execution state. It requires matching
Active HRR1, HGR1 provenance and a complete matching HGA1 attempt. Source snapshot payloads are
not used. Callers must retain original authorization/runtime exclusion and independently validate
the complete fresh hardware fingerprint before execution. The trusted worker/registry cold-start path below connects this core admission. Configured
library Start now selects that route; signed synthetic configured-start/reset checks pass as
described below. Installed Windows recovery remains pending.

Published generations now include a sealed, checksummed 387-byte HGO1 ownership receipt binding
the HGR1 digest, generation/firmware directory identities and each mutable disk's directory,
device/inode and capacity. It is written and synced before generation publication. Both checkpoint
admission and current-system admission recheck these identities. Same-size replacement disks and
missing/damaged receipts reject without replacement or repair; older experimental generations
without HGO1 require explicit recovery. Filesystem copies require identity-aware migration.
Current disks retain private pinned directories, no-follow opens and explicit exclusive ownership;
adoption creates fresh controller state and transfers the lock through final device teardown.

Five current-system tests and four mutable-disk tests cover changed contents/firmware, source-free
admission, ownership transfer, duplicate descriptors, revocation, phase/receipt rejection and
namespace/size/link/mode replacement. These fixtures contain provenance metadata without CPU
payloads and do not prove guest readiness. The signed core diagnostic separately cold-boots real
modified recovery disks with fresh RAM/controller queues, runs both CPUs, persists new virtio
writes and reopens the same mutable resources after teardown. Ninety-one targeted core/host/model tests and strict lint pass. The full signed suite verifies
three independent mutable generations in each of its original and two fresh-process readers;
source-package inventories remain unchanged. Signed direct and registry restore probes also pass
with HGO1 required. Host workflow, installed Windows cold startup, active-generation reboot
supervision and actual viewer recovery remain unfinished.


The private StartGeneration/GenerationStarted worker exchange now cold-boots an Active generation.
It opens the current generation's mutable disk and EFI state, rebuilds fresh hardware, and checks
its complete fingerprint and topology against the saved selection before acknowledging startup.
Every CPU remains held until an explicitly authorized RestoreResume. Original disk and EFI paths
are not opened, and captured CPU/RAM/device execution state is not replayed. Startup commands
remain excluded from public agent dispatch.

Trusted Sessions::start_generation retains original policy and VM namespace identity through
startup, explicit resume, registry publication and actual worker reaping. Cancellation, revocation,
wrong configuration and substituted startup receipts reject without publishing a session. Cold
resume retains the original policy commit guard through acknowledgement and preserves the exact
Active journal instead of rewriting it. Five new session tests and the expanded protocol tests
pass; the primary native/model/restore suite now contains 76 passing tests. Strict all-target lint
passes for model, machine and engine.

The signed worker diagnostic cold-boots with the original disk and EFI directories moved away,
recaptures modified current firmware into the original VM's snapshot namespace, checks current
and original disk contents and the unchanged Active journal, rejects damaged ownership metadata
without fallback, and starts again after explicit fixture recovery. These are synthetic guests.
Installed Windows/open-application recovery, automatic rollback and UI/MCP restore remain
release gates; signed synthetic configured Start/reset verification is described below.


Configured native Windows Start now detects a selected Active generation and routes through the
owned cold-start registry path. The initial private bounded selection read only chooses the route;
the restore owner reacquires operation/runtime/journal exclusion and rechecks the exact selection
and namespace before preparation. Configured generation preparation validates the saved native
Windows record, completed deployment state and coherent worker/firmware assets, derives the same
stable disk identity and CPU/RAM settings as ordinary boot, and detaches installer media. It does
not initialize, open or recreate the original disk or EFI state. Pending/malformed/foreign selection
never falls back to ordinary boot; ordinary preparation still rejects a selection that appears
after an initially empty probe. The Active journal and installation receipt remain unchanged.
Three new process-backed configured-start tests cover an absent original disk, saved boot shape,
unchanged Active state, pending/deployment rejection, corrupted selection preservation and changed
firmware rejection without a worker. Local startup remains allowed with agent access disabled;
agent capture remains denied. Existing startup and restored-session regression tests pass.
These tests use a protocol fixture. Signed raw current-generation worker checks are separate;
installed Windows recovery and the remaining release gates are still unverified or unfinished.
Signed synthetic configured Start/reset verification is described below.


Configured active-generation startup now installs a weak-registry reboot supervisor bound to
its publication epoch, original policy, exact Active selection and original VM-directory inode.
Only guest Reset triggers replacement. The slot stays locked through previous-worker reaping,
reacquisition of operation/runtime/journal ownership, verified configured preparation, held fresh
startup, explicit resume and publication. Namespace/selection/configuration failure preserves
recovery intent and allows explicit retry. Shutdown, requested Stop and a replaced epoch do not
restart. Registry teardown cancels held startup or resume; owned helper tasks retain their leases
through process cleanup. The supervisor does not keep the registry alive while idle.
Four process tests verify repeated reset and viewer-watch continuity, shutdown/Stop exclusion,
failed reboot plus explicit retry, and registry teardown during gated worker startup. The expanded
primary native/model/restore suite passes all 83 tests, and strict all-target lint passes for
model, machine and engine. The new scripts/check/generationstart.py signs an isolated real worker and runs configured
Start with a 1 GiB/two-CPU synthetic guest and a sparse 64 GiB current disk. It observes three real
PSCI resets followed by fresh worker starts through the same viewer watch, verifies requested Stop
stays stopped, checks Active is unchanged and proves original disk/EFI paths are never created.
This probe uses explicitly synthetic HGR1/HGA1/HGO1 provenance metadata without captured CPU/RAM
payloads; it proves configured fresh execution and supervision, not checkpoint replay, a Windows
installation or recovery of open applications. The full installed recovery/rollback/UI/MCP gates
remain open. Trusted restore handoff and prepared generation startup now install this same
supervisor, as described below.


Trusted checkpoint handoff and prepared generation startup now attach reboot supervision inside
authorized registry publication. Publication independently compares the final Active record to
the original selection, including predecessor and rollback. The supervisor retains the original
policy rather than obtaining new local authorization. Configured startup revalidates saved assets
and settings; a prepared handoff retains its frozen helper/boot description and independently
checks namespace, exact Active selection and complete rebuilt hardware through the worker. Reboots
use current mutable disk/EFI and fresh CPU/RAM/device state, never StartRestored/checkpoint replay.
Prepared reboots reject attached installer media without falling back or changing recovery intent.
Three process-backed tests verify repeated agent-restored reboot and viewer continuity without
an installation receipt, off/on revocation during held reboot without reauthorization, and refusal
to carry installer media into cold boot. The signed generationstart check now has a second profile:
an agent-owned prepared generation executes three real PSCI resets through the same viewer watch,
then off/on revocation releases the old-policy runtime and prevents restart. Active remains exact,
and neither original disk/EFI nor an installation receipt is created. Both signed profiles and all 86 primary native/model/restore tests pass,
as does strict model/machine/engine all-target lint. This profile uses synthetic provenance and
fresh execution; initial checkpoint handoff followed by real guest resets, installed Windows/open-app
recovery, automatic rollback, app controls and MCP restore still require their own live evidence.


Trusted restore::prepare now owns stopped-runtime preparation through source/prior component
admission, generation publication and Prepared-to-Attempted selection. It accepts an existing
restore owner, frozen boot description and distinct source/prior snapshot identities; no agent
endpoint accepts these host paths or chooses the rollback artifact. A blocking owner retains the
operation/runtime/journal leases through actual work, checks original policy, cancellation and
pinned VM/generation-directory identity during component reads and storage preparation, publishes
under the original commit guard, then returns the still-owned prepared handoff. Failures preserve
written recovery artifacts; an existing selection rejects without replacement. Automatic prior capture and source shutdown are now connected by the running-source controller
described below; repeated restore/rollback selection remains unfinished. Preparation cancellation has ownership wiring but no live mid-I/O proof.
Host preparation and workers now share runtime::hardware reconstruction. It keeps original-disk
admission separate from current-disk override, checks original authorization during bounded 64 KiB
firmware reads, and rejects symlink/special firmware files through no-follow/nonblocking opens.
The prepared handoff independently rebuilds hardware and fully decodes the prior checkpoint;
source decode and independent APFS disk/EFI preparation use the existing complete generation path.
Three new host tests reject invalid prior component schemas before generation/selection writes,
stale policy, duplicate prior identity, existing selection and foreign boot namespace. Three new
hardware tests cover mid-read authorization failure, symlink/FIFO rejection and current-disk
reconstruction with an absent original path. All 89 primary native/model/restore tests and the
three hardware tests pass; strict all-target model/machine/engine lint also passes.
The signed snapshot controller now calls this production preparation API instead of manually
building/selecting a generation. Its direct and disk-backed registry profiles pass, including held
activation, current-generation capture/restart, damaged-ownership rejection and unchanged original
resources. These synthetic checkpoints do not prove installed Windows/open-application rollback.
User-facing restore, automatic fresh prior capture, repeated restore selection, crash recovery,
rollback/retirement, app controls and MCP restore remain release gates.


Sessions::restore_snapshot now implements the first running-source restore transaction, with a
separate original-generation entry point for agents. It freezes policy before queueing, holds the
session and operation lease, validates the selected package/namespace and refuses existing recovery
intent before altering the source. It pauses a running source, captures and publishes a fresh named
Before restore checkpoint while the source stays paused, confirms capture cleanup, then stops/reaps
the source. Operation ownership transfers directly into the restore owner without unlocking, with
the original source-directory inode checked before opening/creating the journal. Production owned
preparation validates source/prior components and publishes Attempted intent; the existing held
handoff activates the generation and resumes it under the same original policy. The returned
receipt contains source, fresh rollback and generation identities only. No host paths enter an
agent endpoint; UI and remote/MCP restore routing are not exposed yet.
Cancellation drains capture/stop work and retains operation/session ownership through actual exit.
Before shutdown, failed capture undoes only the controller's pause while original policy remains
valid; it never resumes an initially paused or revoked source. Cancellation during preparation
propagates to its blocking owner, which retains leases until work acknowledges it. Unconfirmed
source cleanup retains ownership rather than admitting another runtime. Failures after source
shutdown preserve the prior checkpoint and recovery artifacts; automatic rollback remains pending.
Five process-backed tests verify running/paused capture cancellation, off/on revocation without
resume, missing-source/stale-policy preflight, preserved malformed intent, and cancellation after
prior publication with operation/runtime exclusion through Stop acknowledgement and actual exit.
The signed automatic-agent profile captures a real fresh prior checkpoint, stops/reaps its source,
prepares/activates the selected synthetic guest, keeps the original viewer watch, binds rollback
identity and preserves original disk/EFI. All 94 primary native/model/restore tests and strict
model/machine/engine all-target lint pass. These checkpoints remain
synthetic; installed Windows/open-app recovery, stopped-source restore, repeated restore selection,
automatic rollback, crash recovery and UI/MCP restore remain unfinished release gates.

The Windows library now reports interrupted Prepared/Attempted restore intent as Recovery required,
including when no installation receipt exists. Invalid or unsafe restore metadata marks that VM
for recovery without hiding other rows or changing its files. Active selections report restored
intent only when setup permits system boot; this listing does not validate generation components
or establish desktop readiness. Owned running/paused hardware retains its lifecycle controls even
with damaged restore metadata. Busy operations and foreign runtime owners retain their existing
status. Five recovery-listing tests and five existing record tests pass; strict engine all-target
lint passes. Automatic recovery and rollback remain unfinished.

The trusted restore journal can now select a successor generation from an exact Prepared,
Attempted or Active predecessor. It requires matching VM/configuration, a fresh unattempted
generation and an explicit predecessor link. The previous selection is sealed and synced in an
exclusive per-generation history file before atomic replacement of the current selection.
Conflicting history and stale selections reject without replacement; cancellation after archival
leaves the original selection retryable, while cancellation after replacement retains the new
recovery intent. Six tests cover these boundaries, including abrupt process exit without owner
destructors and subsequent retry. This is a journal foundation: full checkpoint admission, owned
successor preparation, automatic rollback and UI/MCP recovery still require integration. Metadata
fixtures do not prove usable guest recovery or open-application restoration.

Trusted prepare_successor now connects owned blocking preparation to the successor journal. It
requires the exact saved predecessor, reconstructs compatible hardware, fully decodes the source
and recoverable prior checkpoint, and publishes fresh disk/EFI generation resources before selecting
and marking the successor Attempted. Cancellation retains leases until blocking work acknowledges
it; invalid components leave predecessor intent unchanged. The signed controller restores its saved
prior checkpoint through this production API and held registry startup, verifies a fresh Active
generation with the predecessor link, and retains prior history/resources and the original viewer
watch. All three signed controller profiles, 25 targeted tests and strict engine all-target lint
pass. This synthetic guest check does not establish installed Windows/open-app recovery. Hardware
reconstruction still requires the original disk path; automatic failure-triggered rollback,
stopped-source/repeated user restore, crash-recovery controls and UI/MCP routing remain pending.

Successor preparation and checkpoint worker admission now rebuild hardware without opening the
original mutable disk or EFI directory. A pinned single-package inspection admits manifest identity;
the checked block metadata supplies a bounded temporary empty controller shape with the saved
capacity and authorized disk identity. Recomputed firmware/CPU/RAM/device configuration and exact
layout must match the original authorized fingerprint. Full checkpoint/image decoding still runs
before writable recovered disks attach and any guest executes; the temporary shape is never a
guest disk. Initial preparation retains its original-hardware admission path. The signed successor
check moves both original mutable paths away, completes prior-state recovery into a fresh Active
generation and verifies unchanged retained originals, history and viewer continuity. All three
signed controller profiles, 44 targeted tests and strict model/machine/engine lint pass. Automatic
failure-triggered rollback, installed Windows/open-app recovery and user/agent routing remain open.

Prepared restore admission now makes one automatic saved-prior recovery attempt when handoff fails.
It retains original authorization, checks the VM namespace/session generation and exact failed
selection, and reacquires operation/runtime ownership only after failed-worker ownership releases.
It fully prepares and starts a fresh successor from the saved prior checkpoint; the failed
generation/history stays preserved. The requested restore still returns its original error with
the recovery outcome. Revocation, cancellation and changed ownership never authorize a fresh
grant, and failed recovery does not recursively alternate checkpoints. Four new tests verify
actual-exit exclusion, off/on revocation, cancellation and foreign prepared receipts; all 26 targeted
tests and strict model/machine/engine lint pass. The signed automatic profile rejects one admission
before worker startup, then verifies real saved-prior recovery, fresh Active identity, retained
failed history and the original viewer watch without original mutable paths. Its diagnostic wrapper
waits for successful child exit but does not establish delegated-worker cancellation behavior.
Failures during initial preparation, later guest failure, host-crash recovery and installed Windows
open-application rollback still need integration/verification; UI/MCP recovery remains pending.

Running/paused restored VMs now support another restore through the same agent transaction. An
exact Active predecessor with matching owned hardware is frozen before capture and rechecked with
the original policy/session generation throughout capture and stop. Pending or incompatible intent
rejects before pausing. Each transaction captures a fresh named prior checkpoint, stops/reaps its
source and prepares a fresh successor while retaining predecessor history/resources. The signed
automatic profile performs two consecutive agent restores, verifies distinct generation/prior
identities and predecessor linkage, then passes explicit prior recovery and failure-triggered
rollback without original mutable paths. Two new tests cover pending/incompatible preflight and
cancelled repeated capture preserving the active source/selection. All 28 targeted tests and strict
model/machine/engine lint pass. This synthetic check does not establish installed Windows/open-app
recovery. Stopped-source restore, preparation-failure rollback, crash recovery and UI/MCP routing
remain unfinished.

The current-generation worker can now capture a stopped prior state while its cold hardware stays
held before guest execution. Only that cold admission gate permits snapshot start; checkpoint
admission still rejects capture before activation, and ordinary resume/input remain blocked. The
first capture extends the initial authorization deadline to a fixed five-minute limit established
at admission, without allowing repeated requests to extend it further. A signed probe publishes and
fully decodes the held checkpoint, verifies current disk/EFI bytes and zero unused RAM, then proves
explicit release executes firmware that writes a marker there. Original mutable paths remain absent.
The diagnostic initially checked device-tree RAM and was corrected to use unused RAM; it did not
demonstrate early guest execution. Signed cold-current and checkpoint-admission profiles pass,
along with 22 targeted tests and strict model/machine/engine lint. The stopped-source host restore
transaction is described below; original-disk cold hold still needs integration.

Stopped VMs with an Active restored generation now have an owned restore transaction. It starts
current hardware held before guest execution, captures a fresh named prior checkpoint, stops and
reaps the worker, and transfers continuous ownership into a fresh successor and recoverable
activation. Cancellation and the original access policy remain effective through cleanup; uncertain
cleanup retains ownership. The normal restore API selects bundled assets and saved configuration
for idle/stopped sessions and transfers its existing operation lock without a release/reacquire gap,
without accepting agent boot paths. Five process-backed checks cover
startup/capture cancellation, off/on revocation, stale API policy and competing runtime ownership.
The signed synthetic probe verifies the trusted stopped transaction, retained predecessor history,
viewer-watch continuity and absent original disk/EFI paths. Successful configured API routing and
installed Windows/open-application recovery remain unverified. Stopped restore before any restored
generation, preparation-failure rollback, crash recovery and UI/MCP restore remain unfinished.

Snapshot component files now preserve zero-filled regions as sparse holes while retaining their
complete logical length, bytes and checksums. A filesystem test verifies fragmented mixed writes,
reduced allocation, private permissions and exact checked-reader contents after reopening. Existing
package corruption, authorization and publication checks pass. The RAM codec already omits whole
zero-filled 64 KiB chunks; package sparse writes apply to zero regions emitted by component encoders.
Populated guest memory and full logical disk scans still require performance verification.

Snapshot authorization now buffers each fresh bounded record read. Full-size profiling found the
previous JSON reader issued byte-by-byte file reads at every hashing check. The original policy
generation, revocation checks, size limits and validation remain in force; records are not cached.
A trailing-buffer regression verifies malformed suffixes invalidate the request and its clones.
All 45 targeted policy/controller/restore tests and strict model/machine/engine lint pass. This
addresses record-reading overhead; full-size disk-scan timing and configured restore still need
successful live verification.

Authorized agents can now invoke experimental native Windows restore through vm.restore. The
host facade sends saved checkpoint identity and the original access generation to Hopper's owned
agent service; boot paths are never accepted. Restore retains a fresh named prior checkpoint and
returns source, prior and fresh-generation identities. Disconnect, pipelining or revocation cancels
the request while owned cleanup continues. Timeouts can leave recovery state, which must be
inspected before retrying. Stopped VMs currently require an Active restored generation; other
guest families retain their offline disk behavior. Four socket checks and an external-owner MCP
cancellation check pass, along with all 28 targeted engine/MCP tests and strict lint across the
five affected crates. Signed synthetic probes verify successful restore through both HPA1 and
real MCP stdio, retained viewer watches and subsequent prior recovery/automatic rollback. Installed
Windows/open-application recovery, full-size performance and UI recovery remain unfinished.

New disk captures use HDS2: a size-bound checksum of ordered nonzero 64 KiB content chunks.
Filesystem holes and allocated zero bytes produce the same checksum. Verification skips holes;
filesystems without extent queries fall back to reading all chunks. Existing HDS1 records keep
their original full-file verification. Prepared disk comparison skips only regions that are holes
in both files and still compares every remaining byte. Extent queries preserve descriptor cursors.
Tests cover an independent checksum vector, allocation-independent results, a sparse 64 GiB disk,
new writes into old holes, cursor preservation, legacy reads and unknown-version rejection.
All 94 targeted machine/engine/MCP tests pass, with five existing ignored cases; strict five-crate
lint passes. A signed synthetic probe with 1 GiB RAM and a sparse 64 GiB disk now verifies repeated
running restore, the normal configured stopped-restore API, explicit prior recovery and automatic
rollback after admission failure. Original mutable disk/EFI paths are absent during stopped
recovery. A second full-size signed profile passes through real MCP stdio, the host facade and HPA1,
then completes the same recovery checks. The older full-scan diagnostic ended with restore/rollback
worker timeouts and retains its failed fixture; HDS1 verification still performs a full scan.
This closes the earlier synthetic configured-facade gap; populated Windows/open-application recovery,
preparation-failure rollback and crash/UI recovery remain separate gates.

Running native Windows restore now verifies every component body against its manifest-bound
checksum before pausing or capturing prior state. Reads and hashing stay bounded to 64 KiB chunks;
the owned job retains session/operation exclusion while blocking reads finish, including caller
cancellation. Corrupted bodies with unchanged footers, damaged footers, truncation and trailing
bytes reject without pausing/stopping the current VM or publishing a prior checkpoint. Three
machine checks and a process-backed engine regression cover corruption and revocation, and all
48 targeted machine/engine/MCP checks plus strict five-crate lint pass. A signed 1 GiB RAM/64 GiB
sparse-disk agent probe verifies damaged-body rejection with a running source and unchanged
inventory, then successful MCP restore, repeat/stopped/prior recovery and automatic admission-failure
rollback. This precheck does not decode component schemas, verify separately stored disk images or
freeze files against later changes. Full validation before source stop is separate from this
integrity precheck; late-change and preparation-failure recovery remain unfinished.

Running restore now fully decodes both the requested checkpoint and its freshly captured prior
state before source stop. Hardware shape is rebuilt from saved checkpoint metadata and independently
matched to the current configuration; disk images use their recorded content checksums. Validation
retains original policy, namespace/generation checks and operation/session ownership. Failure retains
the named prior checkpoint and resumes only a pause performed by this restore. A previously paused
VM stays paused. The process-backed invalid-hardware check covers both states; all 37 targeted
engine/MCP checks and strict five-crate lint pass. A signed 1 GiB RAM/64 GiB sparse-disk profile
rejects invalid checksummed schemas and altered captured disk data before stop, retains fresh prior
checkpoints and resumes the source. It then passes real MCP restore, repeat/stopped/prior recovery
and admission-failure rollback with full target/prior decoding enabled. These reads do not freeze
saved files, so late changes and failures after source stop still require preparation-failure recovery.
Installed Windows/open-application verification and the remaining release gates stay open.

Trusted restore preparation now has a retained-owner result. Ordinary failures return the original
operation/runtime/journal owner alongside the error instead of forcing recovery to reacquire those
locks. The blocking stage borrows that owner; cancellation still retains it until actual work ends.
Existing preparation APIs keep their previous error behavior. Three regressions verify retained
exclusion, unchanged predecessor selection and off/on revocation without refreshing authorization.
All 30 targeted engine checks and strict five-crate lint pass. A signed 1 GiB RAM/64 GiB sparse-disk
profile verifies failure with unchanged selection and all three locks held, then prepares/starts the
saved prior using the same owner without original mutable paths. The recovery path below also
supports a target that remains corrupt through recovery startup.
Callers must inspect durable selection before continuation. Automatic recovery uses this retained
ownership path as described below.

Running restore now makes one automatic prior-state recovery attempt when preparation fails after
source stop. It retains original ownership, policy and generation checks, validates the durable
selection, fully admits the saved prior and publishes an independent "Before recovery" checkpoint.
Fresh recovery hardware uses the prior plus that backup; the damaged target is preserved and need
not validate. Recovery reports the requested restore's original failure, and failed recovery does
not alternate recursively. Cancellation and revocation reject further work. All 39 targeted
engine/MCP checks and strict five-crate lint pass. A signed 1 GiB RAM/64 GiB sparse-disk HPA1 test
injects target corruption on the observed source-stop transition, verifies automatic prior startup
with the target still corrupt, independent backup/selection, retained runtime ownership and the
original failure response. A trusted stopped test also recovers without original mutable paths.
The full real-MCP/repeated/stopped/prior and admission-failure rollback profile passes. Recovery
has paused-state support as described below. Installed Windows/open-application recovery,
crash recovery and the remaining release gates still require verification.

Worker-startup failure recovery now uses the same independent prior-backup path. It waits for actual
failed-worker cleanup, rechecks original policy, namespace, generation and exact failed selection,
then prepares the saved prior without validating the damaged target again. Recovery remains a single
attempt and reports the requested restore's original failure. A new process-backed check changes
selection during failed-worker cleanup and verifies rejection without replacement, new checkpoints
or another startup. All 33 targeted engine/MCP tests and strict five-crate lint pass. The signed
diagnostic now launches the real worker against late-corrupted checkpoint data; its old rejection/
forwarding wrapper has been removed. The full signed 1 GiB RAM/64 GiB sparse-disk run passes, including
actual worker rejection and automatic prior startup while the target remains corrupt. Installed
Windows checks remain open; paused-state support follows below.

Native Windows restore now retains an already-paused source through normal restoration and
automatic preparation/startup recovery. Private activation commits the selected generation and
keeps its CPU hold; explicit resume releases it later. Held restored hardware can take a new
checkpoint without executing guest instructions, retaining its admitted CPU/clock state and pending
actions while capturing current memory/devices. Cancellation, policy changes and mismatched replies
are checked. Signed 1 GiB RAM/64 GiB sparse-disk checks verify paused preparation recovery and normal
restore without guest RAM changes, including a 31-second hold beyond admission expiry; RAM changes
only after explicit resume. Both CPU/PMU/clock records remain identical in independent held captures.
A corrected diagnostic watch-order error required continuing the preserved fixture; the continuation
passes real-worker corrupted-startup recovery in both running and paused modes without original
mutable paths. Targeted tests and strict five-crate lint pass. A fresh combined signed rerun and
installed Windows/open-application proof remain pending; failed diagnostic fixtures are preserved.

Clock settings and alarm/control state have a checksummed HRT1 record, required by the
HSM2 complete inventory. Older experimental HSM1 packages reject with an explicit
migration error and remain preserved; restore never silently invents missing clock state.
The signed two-CPU guest now performs real clock reads/writes, and fresh-process recovery
checks that its one-hour guest-only offset survives. Unit tests cover offset/counter
wrap, identity/access bounds, control reset, masking/clear, host clock reversal and record
damage. A repeated recovery check failed without captured stderr being reported; its
cause remains unexplained. The runner now emits those errors and preserves failed
fixtures. A diagnostic rerun and three additional two-restore runs passed; these passes
do not establish that the intermittent failure is fixed.

A subsequent full-checkpoint run captured the natural failure: a PMU read reached the
neighboring write checkpoint despite retry resets. Read and write code slots now loop
back to their own access after HVC, with a guard before the next slot. A signed test
that continues past HVC fails with the old layout and passes with the isolated layout,
checking reads, writes and original CPU-context restoration. This prevents that fallthrough
from reaching a different operation; the underlying missed-exit cause remains unproven.
Eleven full diagnostic runs pass with two fresh-process restores each, independent
recapture and authorization/immutability/rejection checks. Machine tests, strict lint and
signed interruption/fallthrough tests pass; production snapshot release gates stay open.

Installed Windows recovery with the isolated code slots also passes: the captured
lock screen matches immediately after fresh-process restore, guest input opens sign-in,
and disk I/O continues without device faults. The original installed guest, disposable
source and snapshot metadata remain unchanged. Open-application preservation and
production snapshot integration remain unverified.

A new installed Windows run visually verifies October 12 at boot, identical held pixels
after fresh-process recovery, and sign-in after guest input. Disk requests continue with
no reported device faults; snapshot/source metadata remain unchanged. Full machine tests,
strict all-target lint and signed PMU fault/timer/cancellation diagnostics pass. Open-app
preservation, production snapshot/rollback/clone integration and the other release gates
remain unfinished.

CPU capture also preserves the advertised hardware breakpoint/watchpoint banks and
debug controls. Restore compares a feature fingerprint before changing CPU state;
configurations advertising SVE or SME reject until their wider state is covered. The
signed probe verifies a nonzero guest-written debug address survives VM recreation and
an incompatible PMU configuration rejects without changing the fresh CPU. Tested PMU
register reads fail through the SDK even with Hopper's PMU feature configuration, so
performance-counter preservation remains unfinished.

The signed `pmu` diagnostic verifies guest-only cycle-counter reads and writes using
immutable read-only code. This host reports zero event counters; its cycle counter
continues across a 100 ms CPU-owner hold with counting disabled. Snapshot preservation
therefore needs coordinated architectural access and clock rebasing. This diagnostic
does not connect these accesses to installed guests or a snapshot endpoint.

The expanded probe verifies independent PMU state on two CPU-owner threads and compares
seven writable settings after architectural capture, CPU destruction and reconstruction.
On the verified host all seven reset: control, counter enable, selector, user access,
interrupt enable, cycle filter and overflow status. The cycle value also resets. A
2^40-tick shift of the architectural timer offset does not rebase PMCCNTR, so the existing
frozen timer projection cannot preserve this counter by itself. A separate signed SDK
probe also finds that enabling debug-register traps does not trap these PMU accesses.
These checks expose missing preservation; they do not establish snapshot recovery.

The separate `hypervisor::pmu` primitive now preserves the currently exposed cycle-only
PMU configuration through fixed instructions in a private immutable code mapping. It
temporarily masks exceptions and disables stage-one translation, then reinstalls the
full architectural checkpoint and debug-trap configuration. Private exception vectors
prevent unexpected EL1 faults from entering guest handlers. The access window has a
100 ms exit deadline; failed context restoration requires VM disposal. Feature mismatch
and event-counter configurations reject. The signed probe verifies all seven settings
and the cycle epoch survive complete VM teardown, including an MMU-enabled source whose
scratch address has no stage-one mapping. It checks architectural state, interrupt bytes,
guest RAM, occupied-address rejection and incompatible-feature cleanup. Full machine
tests and strict all-target Clippy pass. Coordinated
runtime capture/final resume rebasing, installed Windows verification, durable snapshots
and worker/agent integration remain pending. This primitive does not enable snapshots.

The signed PMU fault/timer check executes an undefined instruction and an infinite loop
inside the temporary context. The fault reaches the private HVC vector instead of a
guest handler that would write RAM; the loop exits through the 100 ms watchdog. Both
restore architectural state and preserve the captured interrupt bytes and guest memory.
Enabled physical/virtual timer controls and the original timer-delivery mask survive
capture/restore with future and already-due deadlines, including a routed pending physical
timer interrupt. A deadline-crossing check starts with guest IRQs enabled, passes the
deadline during temporary execution, and verifies compares/controls and original IRQ
enable survive while the due interrupt remains pending and no guest handler runs.
The live test is ignored by ordinary Cargo runs because
it requires a Hypervisor-entitled executable; this command builds, signs, verifies and
runs it explicitly:

```sh
python3 scripts/check/pmu.py
```

Primary checkpoint collection now completes its disk/RAM/device/GIC cut before sampling
the shared frozen clock and allowing CPU collection. Blocking disk clones therefore no
longer age an already-frozen epoch before its interrupt cut exists. The signed private
phase check deliberately delays primary capture, verifies CPU capture stays unavailable
until success, checks the frozen epoch follows the completed cut and rejects primary
failure. The complete signed reconstruction/capture regression passes after this ordering
change.

Native coordinated checkpoints now include each CPU owner's cycle-only PMU state.
Capture prepares a private immutable instruction mapping outside guest RAM; normal
uncaptured startup does not allocate it. Every CPU waits for the primary cut before
performing its own PMU read. Any PMU capture-context failure stops the runtime, preventing
resumption of a partially restored owner. Reconstruction retains PMU state while initially
held and restores it at final Continue after rebasing timers to the shared resume epoch.
The signed `pmurecovery` probe seeds distinct settings and cycle values on two real guest
CPUs, captures/destroys the source and independently reconstructs it twice. After a 750 ms
initial hold, both CPUs retain their settings/cycle epoch and both enabled 500 ms timer
types retain their delay, then expire after execution resumes. It rejects cycle aging
beyond the probe's 2^30-cycle margin; maximum host-load timing remains unverified. Complete
signed capture/reconstruction regressions pass. An installed Windows round trip with this
runtime restores matching lock-screen pixels, accepts synthetic Space to reach sign-in
and continues disk I/O for thirty seconds. Original VM storage remains unchanged. This
does not establish Windows-specific performance-counter semantics or open-application
preservation. Durable snapshots, worker/agent endpoints and application restoration remain pending.

```sh
CARGO_INCREMENTAL=0 cargo build -p machine --example pmurecovery
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/pmurecovery
codesign --verify --strict target/debug/examples/pmurecovery
target/debug/examples/pmurecovery
```

```sh
CARGO_INCREMENTAL=0 cargo build -p machine --example pmu
codesign --force --sign - --entitlements assets/machine.entitlements target/debug/examples/pmu
codesign --verify --strict target/debug/examples/pmu
target/debug/examples/pmu
```

Frozen CPU clock restoration now reinstalls the guest counter offset before enabling
saved timers. A signed probe recreates a VM across a 750 ms interval, verifies both
enabled 500 ms physical and virtual timers retain their delay, then verifies they
expire afterward. Apple's native GIC shifts both counters with this offset, so their
compare values stay in the saved guest epoch. Counter-frequency compatibility,
multi-CPU coordination, PMU rebasing and full snapshot integration remain open.

CPU capture now connects to a runtime-owned group during coordinated pause. Each CPU
captures on its owner thread with a shared clock epoch; one bounded request is admitted
at a time. Cancellation, resume and runtime retirement invalidate unfinished captures.
Signed probes verify repeated two-CPU captures with stable RAM/registers, shared timing,
cancellation and generation exclusion, single-CPU capture, error cleanup and normal
pause/resume behavior. The checkpoint now also collects RAM banks and GIC state;
remaining devices, disks, PMU state and full recovery publication still need integration.

Native pause now drains secondary device traps and applies their replies before
acknowledgement, avoiding half-applied operations in CPU captures. Queued CPU starts
also finish before the parked-owner checkpoint. A signed two-CPU capture probe now
performs real UART writes alongside stable capture/resume checks; pause and CPU_ON/OFF
probes pass. Full VM snapshot integration remains open.

CPU checkpoints also preserve software power and debug-lock state. Restore rejects
incompatible affinity/executable topology, debug counts and pending starts before
changing software state. Signed capture verifies a real guest-written debug unlock
and a parked secondary CPU; pure checks cover restoration and rejection without
mutation. Rebuilding runtime owners, RAM/devices/disks and full snapshot recovery remain
unfinished.

Native RAM capture now uses a private read-only Mach copy-on-write mapping that owns
its pages independently of VM hardware. Reads are bounded to 64 KiB. The signed `ram`
diagnostic verifies repeated captures of 32 MiB of populated memory, concurrent capture
reads during guest writes, host/DMA write isolation, survival after VM destruction and
guest reads after copying captured RAM into a fresh VM. Local captures took roughly
2–4 ms; this does not establish timing at supported maximum RAM sizes. RAM capture is
part of the coordinated CPU/GIC checkpoint. Remaining device/disk capture and durable
recovery are unfinished; the native snapshot endpoint remains unavailable.

Coordinated capture waits until the requesting callback and interrupt updates finish
before arming CPU collection. Further callbacks and interrupt updates are deferred while
collection is in progress, within its five-second deadline. The primary owner collects
power/debug state, firmware/NVRAM/RAM mappings and opaque GIC state under that boundary.
A signed probe verifies callback updates enter the capture, repeated captures remain
immutable across resume and teardown, and the collected GIC state restores after all
fresh CPU owners exist. Pause, CPU power-cycle and frozen-timer regression probes pass.
Remaining devices, disks and complete VM restore remain unfinished.

The coordinated checkpoint now includes flash command/protection state and the UART's
registers and receive FIFO. Flash reconstruction takes the matching captured NVRAM
bytes and preserves pending word, buffered-write, erase and lock operations. Pure checks
continue every command boundary and verify independent reusable captures and size
rejection. A signed guest probe pauses halfway through a buffered write, lets the original
guest finish, then independently completes the captured write after VM teardown. It also
verifies guest-written UART configuration survives. Virtio backend state and disk
recovery remain unfinished; complete VM snapshot/restore is still unavailable.

PCI transport capture now includes configuration/BAR probes, negotiated features,
queue registers and ring positions, ISR/MSI-X state and diagnostics, bound to each
device slot in the coordinated checkpoint. Unfinished queue requests reject capture.
Restore validates queue geometry and saved progress against matching RAM before any
destination mutation. Checks cover 16-bit index wraparound, reusable captures, pending
legacy interrupts and a masked MSI delivered once after restore/unmask. The signed
group probe restores a configured keyboard transport from retained COW RAM after the
source VM is destroyed. This restores transport state only: backend contents and
disk/media binding still need integration before a complete VM can be recovered.

Input and guest-serial backend contents now accompany their PCI captures. Input
reconstruction preserves selected capabilities, LEDs, held keys and pending events;
serial reconstruction preserves the named port's negotiation, control replies and
bounded input/output bytes. Complete device restore validates input kind or serial
port name before changing its destination. Checks verify reusable reconstruction,
pending delivery and held-key release, both serial directions, queued control replies
and mismatched-device rejection. The signed group probe reconstructs the keyboard from
a coordinated capture after source teardown and exercises pending events and releases
against a private copy of its RAM region. Host leases and authenticated sessions are
not reinstated. Storage backend state and full VM restore remain unfinished.

Graphics capture now preserves resources/backing spans, published frames, unpublished
resource pixels, scanout, cursor and active/pending linear-framebuffer state. Pixel,
backing and frame buffers share immutable ownership across capture; later resource/frame
writes make fallible private copies. Restore validates display configuration and guest
backing/layout ranges before replacing the destination. Checks cover source mutation,
reset and destruction, repeated recovery, unflushed pixels, cursor state and invalid
memory/configuration rejection. The signed group probe reconstructs a captured framebuffer
after source pixels change and the source VM is destroyed. This is snapshot work for the
existing graphics devices; accelerated graphics and complete VM restore remain unfinished.

Block captures now retain private APFS copy-on-write images cloned from the open source
file descriptor. Writable disks are synced first. Recovery reconstructs capacity, identity,
read-only and writeback state into a fresh private clone; existing target contents remain
untouched. Checks verify sparse allocation, source-path replacement and deletion, repeated
independent recovery, read-only enforcement and lock cleanup. The signed two-CPU group
probe captures a disk with completed queue progress, changes its sectors, destroys the VM,
then recovers and reads the old sectors through restored PCI queues, repeatedly. Full machine
tests pass. Images remain ephemeral: durable publication, optical recovery, full runtime
reconstruction and application-preserving VM restoration are unfinished. Synchronous disk
sync/clone can delay the owner queue; maximum-size and I/O-pressure pause bounds are
unverified. Native restore endpoints remain unavailable; experimental capture/listing are described above.

Optical captures now retain independent read-only APFS images and the controller's
negotiated command/sense sizes, pending sense, inserted/ejected state and diagnostics.
Recovery uses fresh read-only descriptors with independent positions, never the original
installer path. Checks verify path replacement, source mutation/deletion, reusable recovery,
write protection and capacity rejection before destination mutation. The signed two-CPU
probe reconstructs optical media from a coordinated checkpoint after VM teardown, retrieves
its pending error, verifies it remains ejected, reinserts it and reads the captured sectors
through the restored PCI queue. Storage is still ephemeral; durable publication and full
VM/runtime restoration remain unfinished.

In-process native recovery now reconstructs fresh firmware/NVRAM/RAM mappings, flash/UART,
software power/debug state and all present PCI backends before recreating CPU execution.
Every CPU exists before opaque GIC restoration; each owner restores its architectural state
and acknowledges it while held. The first callback receives paused hardware. Continue applies
one fresh shared clock epoch before releasing the owners. Parked CPUs remain parked and can
subsequently start through guest CPU_ON. Stop/error before that release tears down the fresh
hardware without guest execution. Signed probes verify repeated two-CPU recovery with actual
continued guest RAM loops and UART traps, restored disk/optical queue I/O, single-CPU execution,
parked CPU startup, invalid topology/affinity/GIC rejection and failure/stop cleanup. Existing
pause, power-cycle and frozen-timer probes pass, as do full machine tests and strict Clippy.
This is a reconstruction foundation: memory is copied in bounded chunks and storage remains
ephemeral. PMU handling, durable snapshot publication, worker/agent endpoints, recovery of
host-side pending actions and live desktop application preservation remain unfinished.

Checkpoint state now retains the bounded UART tail and pending boot-key action as a relative
delay. Recovery does not dispatch the action while held; first Continue starts its saved
remaining delay. A signed probe verifies one complete press/release after VM teardown and
1.2 seconds of held recovery. Pure checks cover absent/overdue actions, reusable deadlines
and tail/delay bounds. Restored runtimes can now capture again after first Continue and a
later coordinated pause. Their initial staged pause rejects capture requests until CPU
owners are released. A signed two-CPU probe captures a restored VM, destroys it, repeatedly
recovers the new checkpoint and verifies continued CPU execution plus independent disk/media
I/O without changing the original checkpoint. Full machine tests and strict all-target Clippy
pass. Durable publication, PMU handling and live desktop application recovery remain unfinished.

Memory recovery now uses `mach_vm_copy` into a fresh writable private mapping before
Hypervisor admission. Captured mappings retain their maximum read-only protection; writes
in either recovered VM remain independent. Firmware/NVRAM/RAM restoration uses this path;
flash command emulation still reconstructs its separate bounded byte bank. Signed checks
verify populated 32 MiB recovery, guest and host/DMA writes, occupied-range rejection,
repeated recovery, recapture and survival after capture-owner teardown. Sparse 1/8/64 GiB
probes verify guest reads/writes at both boundaries and a hole without materializing the
whole range. A separate native region probe observed COW sharing after Hypervisor mapping.
The 64 GiB probe exposed the SDK's default 36-bit physical range being too small once RAM
starts at 1 GiB. Native configuration now explicitly selects up to 40 supported bits and
validates every mapping's range before allocation. Hosts with smaller address widths retain
that bound. Signed full reconstruction, pause, power-cycle and frozen-clock probes pass,
as do full machine tests and strict Clippy. These sparse checks do not establish populated
maximum-size or pressure timing, desktop application preservation or durable snapshots.

A signed `desktoprestore` diagnostic now exercises an installed Windows ARM64 guest using
private APFS copies of its disk and firmware-variable store. With the updated address-width
configuration, Windows booted to its lock screen. The probe captured running state, destroyed
source hardware, reconstructed fresh hardware with identical held pixels, resumed for thirty
seconds and delivered a guest-only Space press/release. Recovered frames visibly show the
sign-in screen; disk requests advanced from 15,233 to 18,271 without device faults. The original
diagnostic disk's inode, size and modification time remained unchanged. This verifies one real
Windows lock-screen recovery round trip, not open-application preservation, PMU correctness,
durable storage, agent endpoints or the full app/viewer flow. Lower-address-width hardware
and populated maximum-memory/pressure behavior remain unverified.
