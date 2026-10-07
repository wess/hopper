# Hopper

Repository guidance for agent sessions.

## Desktop virtual machines

User VMs are managed by `engine::machines`, separate from the managed Docker VM.
The app routes Windows ARM64 through the owned native runtime. Linux and macOS
still use the Lima/VZ prototype. Previous QEMU/swtpm Windows instances remain
preserved for migration and are not started through the native path. macOS and Windows
guests are experimental. Release hosts remain
Apple silicon macOS 26+.
Each VM has a dedicated viewer. Agent access defaults on for new VMs and clones
inherit it. Native Windows key chords and pointer actions use connection-scoped input leases shared
with the viewer. Guest tools and composed text input remain pending. Leases are tied
to both the worker generation and a persisted agent-policy generation. MCP lists and creates
Windows through the native facade. Screenshots connect to the app-owned native registry;
other Windows tools return an explicit unavailable error after authorization, preventing
fallback to the previous runtime. The app hosts a same-user private Unix socket under
`machines/agents/native.sock`; HPA1 accepts status, capture and leased control. Headers are limited to
4 KiB, frames to 64 MiB, connections to 16 and buffered captures to one. Policy is rechecked
before dispatch, after completion and during pixel delivery. Peer credentials and private
socket/directory ownership are checked on both sides. The service does not retain the VM
registry while idle, cannot accept boot paths, and is removed on host teardown.
Disconnected requests cancel while the worker protocol drains. Inside-guest connections,
remote lifecycle and guest-tool transport remain pending.
Native input cannot bypass its lease through the public request API. An owner is invalidated
on cancellation, policy revocation, pause/stop/restart or five seconds of inactivity. Its
exclusive gate remains held through asynchronous held-key/button release; cleanup never
crosses worker generations. Competing claims check the gate before waiting for the VM
session. Release failure blocks reuse in that generation until the worker is replaced.
Idle input leases retain only weak registry references. Viewer focus loss/drop and queue
overflow close the lease; reconnect waits for prior cleanup and cannot release another
owner's keys. Remote control accepts at most 64 input events per packet, refuses pipelining
and closes ownership on malformed frames, disconnect or cancellation. Actual policy
changes increment a persisted counter; off/on toggles invalidate existing agent leases
and operations. Exhaustion still permits revocation and prevents re-enabling access.
Prototype MCP operations recheck the persisted access setting, use per-VM
cross-process locks, and expose only guest files/input. Snapshots require a
stopped VM and use APFS copies, with a rollback snapshot on restore.

The major release replaces desktop VM lifecycle and viewers with owned runtimes:
direct Virtualization.framework for Linux/macOS and Hypervisor.framework for Windows.
Do not add QEMU to the replacement. `PRODUCT.md` contains the full release gates.
The native `machine` crate supplies the `hoppervm` worker, bundled with its firmware
and Hypervisor entitlement. Its bounded parent-only pipe protocol lives in
`model::native` and `machine::ipc`; host paths in Start are never an agent API.
`engine::machines::native` owns the asynchronous parent client. It serializes requests,
drains replies after caller cancellation, and tracks worker state independently of viewers.
`Host::native_machines` retains native sessions independently of viewer watches. Runtime
ownership locks survive cancelled startup and registry teardown until process cleanup.
Guest operations recheck the persisted policy at dispatch and before returning captures.
Configured startup verifies bundled firmware artifacts and derives private VM paths
from the persisted record under the runtime locks. Deployment requires an unwritten
sparse disk; system boot detaches setup media. The Rust setup-media builder publishes
a private, checksummed per-VM bundle and preserves existing data on failure. Startup
checks its VM identity and image checksum before deployment. `Sessions::deploy`
inspects a supplied installer read-only, selects Professional and recovery capacity,
preserves guest credentials, builds media and allocates a new sparse target before
launch. Retry checks also match installer and deployment-plan hashes. Preparation
retains ownership through cancellation; abandoned preparation does not launch hardware.
Native Windows resources bundle the two media tools, relocated libraries, pinned
ARM64 drivers and license notices. `Sessions::deploy_bundled` verifies their inventory
and hashes and resolves them coherently for the app and its sidecars. Signing refreshes
the asset manifest before sealing the app. Native catalogue acquisition restricts redirects
to official HTTPS origins, bounds CAB/XML responses and rejects ambiguous ARM64 selections.
It preserves the exact advertised ESD URL: Microsoft's delivery hostname currently uses
HTTP and does not have a matching HTTPS certificate. ESD integrity metadata comes from
the HTTPS catalogue; downloads use no redirects and verify the complete size/checksum
before conversion. Never disable certificate verification or rewrite that hostname.
When no installer is selected, native preparation acquires and converts official media
with bundled tools. Its private atomic cache retains the source CAB, source identity and
ISO checksum, and verifies them on reuse. Partial ESDs survive interrupted transfers;
verified ESDs survive conversion failures and are removed only after complete publication.
Invalid cache data is preserved and rejected. Saving the acquired path shares the agent
policy lock and rereads the record, preserving concurrent access changes. Supplied local
ISOs remain a fallback. Installation progress is private, bounded and atomically persisted
per VM. A weak registry controller observes setup independently of viewers, retains failure
and interruption state, and defers a completed handoff while manually paused. It persists
deployment completion before stopping/reaping WinPE, reacquires runtime ownership under
the operation lease, and boots from the system disk with both DVDs detached. System boot
rejects a wholly unwritten target. Handoff failures retain system-boot intent for retry.
VM watches survive worker replacement; generation changes and publication share one lock
so a stale worker cannot overwrite the current state. A surviving viewer receiver cannot
keep a worker or registry alive. SystemStarted proves hardware allocation only.
The controller follows explicit guest resets through system-worker replacement, with both
DVDs detached and the same disk/firmware identity. Manual system startup installs the same
supervision. Guest shutdown and requested stop do not trigger automatic startup; failed
reboots retain retry intent. Full official ESD conversion and Windows installation/first-boot
verification, live viewer inspection and guest-tool integration remain pending. Existing prototype instances
need migration before native startup. Native registry creation validates Windows resources
and stores policy without invoking legacy helpers or allocating a disk. Its Windows listing
uses records, durable phases and VM-scoped hardware state without Lima, marks previous
instances for migration, and detects external runtime ownership. Runtime probes retain the
operation lease and skip probing during another operation, avoiding contention with startup.
The app and host facade now create/list/start Windows through native sessions. Windows
viewing never falls back to the previous helper. Each VM has one Hopper viewer window;
closing it keeps the VM running. Frame polling is serial and bounded, slows in the background,
and evicts old uploaded images. Guest input has a bounded queue, focus generations, priority
release and cancellation of an in-flight request on focus loss. Closing or losing activation
releases held input and invalidates queued transitions. Pointer mapping excludes letterbox
margins. Startup reports download, inspection, media, disk and boot stages.
CPU/memory/disk creation bounds match native limits. Windows snapshots/clones are
disabled until their native implementation exists. Visual and physical Windows display/input
verification remain pending; synthetic worker checks do not prove Windows desktop readiness.
VM and temporary target leases explicitly unlock on owner teardown; closing a file
alone can retain a flock through a duplicated or inherited descriptor. Do not replace
those leases with bare file handles. The download writer retains its inode lock through
pending asynchronous writes and has a different ownership lifetime.
Dropping the final client stops the worker.
Started means allocated hardware, and Deployed means deployment completed; neither
proves a usable desktop. Direct VZ integration, inside-guest transport, guest tools and
running-memory snapshots remain pending.

## What this is

Hopper is a native desktop app for running and managing containers — a Docker
Desktop replacement written entirely in Rust. The UI is
[gpui](https://github.com/zed-industries/zed) + [guise](https://github.com/wess/guise);
the async layer is Tokio.

It speaks to two kinds of engine. On **macOS** its default managed engine is a
headless Linux VM running rootful Docker. A bundled Lima helper drives Apple's
Virtualization.framework and owns guest provisioning, file sharing, and port
forwarding. VM data lives under `~/.hopper/engine/lima`, isolated from other
Lima installations. Apple's `container` runtime remains an optional backend. On
**Linux** it attaches to whichever of Docker or Podman is installed. Anything
that answers the Docker Engine API (Docker Desktop, Colima, Rancher Desktop, a
remote daemon over TCP) works everywhere as a fallback.

The architecture follows [tables](https://github.com/wess/tables) — a Cargo
workspace layered bottom-up, gpui-free core crates, one Tokio↔gpui bridge.

> Ported from an earlier TypeScript/Bun + React build, and from a hand-rolled
> Virtualization.framework VM that Apple's runtime now supersedes on macOS.
> Nothing in the repo depends on Bun, Node, `butter`, or `basket`. Lima supplies
> the guest boot machinery; Hopper does not build a kernel or initramfs.

## Commands

Rust throughout. No Bun, no npm.

```sh
cargo run -p app                 # launch the app (dev binary: hopperdev)
cargo build --workspace          # build everything
cargo test                       # the whole suite
cargo test -p docker             # one crate
cargo clippy --all-targets       # lint
cargo run -p mcp                 # the stdio MCP server (hoppermcp)
```

Release (macOS `.app` + `.dmg`):

```sh
scripts/bundle.sh        # assemble + sign dist/Hopper.app (CODESIGN_IDENTITY)
scripts/dmg.sh           # package dist/Hopper.dmg
```

## Architecture

A workspace layered bottom-up; each crate depends only on those below it, and
the gpui-free core never imports gpui.

- **`model`** — shared domain types, the wire contract every crate speaks. Pure
  serde; field names stay `camelCase` so `~/.hopper/` files and MCP JSON
  round-trip. One focused module per domain.
- **`store`** — local persistence: JSON documents under `~/.hopper/` (atomic
  writes, corrupt-file backup) and the OS keychain for secrets (single-line
  JSON per key — the macOS keychain corrupts values containing a newline).
  `HOPPER_DIR` overrides the root.
- **`docker`** — the Engine API client and every domain module. `client.rs` is
  hyper-over-transport (`transport.rs` covers unix/tcp/tls/npipe); it resolves
  the endpoint fresh per request (a provider can repoint it at runtime) and
  negotiates the API version down for older daemons. Streaming calls are
  cancelled by dropping the future. `demux.rs` is the stdcopy framing;
  `exec.rs` hijacks the socket for an interactive TTY (needs the
  `Upgrade: tcp` / `Connection: Upgrade` headers or the daemon won't 101);
  `archive.rs` copies files in/out and browses container filesystems.
- **`apple`** — the optional Apple Containers backend. `container` talks to its
  apiserver over XPC and publishes no Docker Engine API (the request to expose
  one was closed as not planned), so `cli.rs` drives the binary and `wire.rs`
  maps its JSON onto the same `model` types the Engine API path produces. Be
  liberal in what you accept there: Apple promises stability only within a
  patch version.
- **`engine`** — the provider abstraction (attach to an engine, or supply one).
  `providers/vm.rs` manages Hopper's private Lima VM; `vm/` holds its CLI and
  profile. Docker readiness, rather than VM boot alone, determines connection.
  `providers/apple.rs` is the macOS engine, `providers/linux.rs` finds Docker or
  Podman (rootless sockets first), `providers/existing.rs` is the
  always-available fallback.
- **`migrate`** — Docker Desktop → Hopper migration (scan + copy).
- **`host`** — the async service facade (`Host`) the UI calls. Owns the Docker
  client, the engine registry, settings, and workspace scoping. gpui-free.
- **`mcp`** — the stdio MCP server; protocol framing plus the Docker tool set.
- **`app`** — the gpui + guise application. `bridge.rs` is the Tokio↔gpui seam;
  `state.rs` the cross-view signal contract; `views/` one module per surface.

### The two backends

`host::runtime::Backend` is an enum, not a trait: the streaming calls take
closures, and `FnMut(LogLine) -> bool` is not object-safe. `EngineCapabilities`
carries what each backend can actually do, so the UI hides what is missing
rather than offering a button that always fails. Apple's runtime has no pause,
no rename, no post-create resource update, no event stream and no healthchecks.

Two traps worth knowing:

- `container system start` reads from stdin when it is not told whether to
  install the default kernel. Hopper runs it with no terminal, so it always
  passes `--enable-kernel-install`.
- Apple renders JSON dates as ISO8601 (`Output.renderJSON` defaults to the
  `.compact` options), not Swift's reference-epoch default.

## Conventions

- Functional style; avoid classes/OO where a free function + plain data will do.
- Keep the gpui-free core (`model`, `store`, `docker`, `engine`, `migrate`,
  `host`, `mcp`) free of gpui — the boundary is `app`.
- File names lowercase, no spaces/dashes/underscores; split by directory, not
  compound names. Small, focused files.
- Pure logic carries the unit coverage (parsers, arg builders, framing, diffing,
  reducers). Integration seams — the `container` CLI, socket discovery, the
  import path — are where the real bugs hide; verify those against a live
  engine, not just tests.
- The workspace version in the root `Cargo.toml` drives releases.
