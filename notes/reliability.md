# Hopper reliability work

Hopper remains a native Rust client with one managed Linux VM on macOS.
The gpui-free engine, transport and persistence boundary is preserved.

## Implemented safeguards

- Migration has a read-only review. Name conflicts block import. Unsupported
  settings and missing volume contents are reported. Imported containers stay
  stopped with restart disabled; source resources are untouched. Image archives
  are removed after each transfer, and the final outcome is separate from the
  bounded progress queue.
- Engine operations capture the backend, endpoint, TLS configuration and API
  version. Changing engines cancels the old event connection. Multi-step
  operations stay on their original engine.
- UI log, stats and terminal streams have owners and are cancelled on route,
  tab, container or engine changes. Logs use a virtual list and retain at most
  2,000 lines / 4 MiB, with 16 KiB per line. Terminal dimensions, cells, escape
  parsing and scrollback are bounded; UTF-8 decoding spans transport chunks.
- Engine API negotiation validates both ends of the supported range
  (1.24–1.44), including the daemon's minimum. Connect, response-header and
  upgrade-handshake waits have deadlines even for long-lived streams.
- New VMs have no implicit home mount. Explicit grants default to read-only;
  overlapping grants and template expressions are rejected. Edits remove
  inherited Lima mounts before applying the grant list. The API socket stays
  private and forwarded host ports bind to loopback.
- Event reconnect uses backoff, jitter, replay cursors and inventory
  reconciliation, including a 30-second refresh and immediate reconciliation
  after recovery even if an event subscription stays silent. Inventory requests share a short-lived snapshot, invalidated
  by events, mutations and selection changes. Cache keys include generations
  so a request finishing after invalidation cannot revive a stale snapshot.
- Build contexts stream through a bounded channel, without buffering the full
  archive or staging another tar file. Worker cancellation propagates through
  backpressure. Dockerfile-specific ignores, ordered negation, rooted patterns,
  symlinks and empty directories are covered, including differential checks
  against Docker BuildKit.
- Default VM allocation is 2 CPUs / 2 GiB, bounded against host capacity with
  headroom. The UI exposes VM memory/disk/load and guest filesystem trim.
  Docker defaults use local rotating logs (10 MiB × 3 per container) and a
  5 GB build-cache target. These are retention targets, not a total disk quota.
- Lifecycle commands have local and OS locks. Stop cancels startup. A running
  VM gets a Docker-readiness grace period before recovery restarts it. Healthy
  status probes use the socket without repeatedly launching Lima. The app's
  Tokio runtime uses two I/O workers, and unchanged health polls do not notify
  UI observers.
- Daemon configuration is validated before saving and by `dockerd` before
  activation. Failed activation restores the previous document. Credentials
  travel over stdin rather than process arguments or reflected error output.
- Settings and workspace writes use private atomic replacement, sync and OS
  locks. Disjoint settings edits merge; conflicting edits fail visibly. Folder
  grants are validated again after merging. Workspace edits detect stale
  records. Settings offers a reload action. State-file reads are size-limited.
- Hopper mutations use one admission lock per engine endpoint across app and
  MCP processes. Conflicts fail explicitly; reads remain concurrent. A private
  journal retains 64 outcomes and marks abandoned work interrupted before a
  later operation. Interrupted mutations are never automatically replayed.
  `docker.recent_operations` exposes the journal through MCP. External Docker
  clients do not participate in Hopper's admission locks.

The native app currently reads Compose files and does not have a local Compose
file editor. The old TypeScript editor described in `notes/compose.md` is not
an active native write path; this work does not add another editor.

## Optional empty-engine suspension

This is off by default. The settings toggle enables a ten-minute interval;
`idleSuspendMinutes` can specify 1–1440 minutes. Any existing container,
including a stopped or paused container, prevents suspension. Before stopping,
Hopper obtains mutation admission, disconnects its event listener, checks guest
API connections and package-manager activity, and rechecks container inventory.
Failures keep the VM running. The feature is deliberately limited to an empty
Docker engine. External CLI requests racing shutdown may fail and require the
user to start the engine again; there is no automatic socket-based wake.

## Guest and dependency update policy

New VMs use a digest-pinned Ubuntu 26.04 image and Docker 29.9.3's static arm64
archive ([security patch notes](https://docs.docker.com/engine/release-notes/29/#2993)), verified against a source-controlled SHA-256 before extraction. There
is no moving `curl | sh` installer or unverified image fallback. The Lima 2.2.1
helper archive is also pinned to a source-controlled digest.

Existing VM disks retain their installed guest runtime. On the next stopped-VM
start through Hopper, legacy remote installers in the profile are replaced;
the new provisioner preserves an existing Docker service and uses the pinned
archive for an unprovisioned or interrupted first boot. A Hopper upgrade must
not silently replace a running Docker installation or its data. Guest Docker
version/digest changes require release review and fresh VM certification;
existing guests need a deliberate, backed-up upgrade or migration. Ubuntu's
signed OS security-update services remain available; Docker's static binaries
are serviced through reviewed Hopper guest-runtime releases. Availability of
the pinned image must be checked before shipping; an expired upstream image
URL requires a verified replacement or mirror, not a floating fallback.

CI now checks the platform core on Linux and Windows in addition to the macOS
workspace, and runs a live BuildKit comparison on Linux. Actions use immutable
commit IDs. Advisory gates reject vulnerabilities, unsound packages and yanked
versions. Unmaintained transitive packages remain visible warnings, including
parts of gpui's platform/font stack and the YAML dependency chain.

`scripts/check/sbom.py` checks declared licenses and emits a CycloneDX 1.5 graph
for locked Rust dependencies plus bundled helper identities. It includes crate
checksums and upstream helper-download hashes. This host inventory is not a
complete guest OS package SBOM. Releases attach it, license declarations, and
SHA-256 sums alongside the DMG. Signing and notarization remain independent
release checks.

## Arcane patterns adapted

Reviewed Arcane main at `fdc14d08376c7d3ea917ee1cdb0e7e2ae342ab1f`.
These are architectural adaptations; no Arcane source was copied.

- [Stats service](https://github.com/getarcaneapp/arcane/blob/fdc14d08376c7d3ea917ee1cdb0e7e2ae342ab1f/backend/internal/container/children/stats/service.go):
  share overlapping requests, bound collection and close owned streams. Hopper
  keeps detailed stats active only for the selected detail view.
- [Compose cache](https://github.com/getarcaneapp/arcane/blob/fdc14d08376c7d3ea917ee1cdb0e7e2ae342ab1f/backend/pkg/projects/cache.go):
  explicit cache validity and isolated returned values. Hopper uses short
  inventory lifetimes plus generation-based invalidation.
- [Updater admission](https://github.com/getarcaneapp/arcane/blob/fdc14d08376c7d3ea917ee1cdb0e7e2ae342ab1f/backend/internal/updater/admission.go)
  and [jobs](https://github.com/getarcaneapp/arcane/blob/fdc14d08376c7d3ea917ee1cdb0e7e2ae342ab1f/backend/internal/job/service.go):
  mutation admission and durable operation outcomes. Hopper uses a smaller
  local journal and conservative endpoint-level exclusion.
- [Filesystem writer](https://github.com/getarcaneapp/arcane/blob/fdc14d08376c7d3ea917ee1cdb0e7e2ae342ab1f/backend/pkg/projects/fs_writer.go):
  atomic writes, conflict preservation and recovery state. Hopper applies
  those principles to its owned preferences and daemon configuration.
- [Published SBOM](https://getarcane.app/sbom): make dependency inventory part of
  delivery. Hopper generates its inventory from the locked Cargo graph.

Arcane's multi-user server, scheduler and browser frontend solve a different
product problem. Hopper does not need those services to adopt these patterns.

## Verification

Use isolated state for verification:

```sh
HOPPER_DIR=/tmp/hopperchecks cargo test --workspace --locked
HOPPER_DIR=/tmp/hopperchecks cargo clippy --workspace --all-targets --locked -- -D warnings
cargo audit --deny unsound --deny yanked
python3 scripts/check/sbom.py dist
```

`scripts/check/engine.sh` provisions its own short-path VM, runs the profile,
restart, volume, bind-mount, port, build-context, grants, daemon-rollback, stats,
trim and idle checks, then deletes only that test VM. Set
`HOPPER_CERTIFICATION_LOGS` to retain logs. It needs the bundled Lima helper and
a Docker CLI with Buildx; the bundled CLI and plugin are used when present.
`.github/workflows/certify.yml` runs it on a dedicated `hopper-vz` Apple silicon
runner. Logs default to `dist/verification/native`. Adding this workflow does not provision or certify that runner.

`scripts/check/resources.py` records process-time CPU deltas and summed RSS
for an app PID, its Lima/VZ hostagent and descendants. Apple's VM XPC service
is outside that tree: identify the matching process and include its PID with
`--extra-pid`. A sample without that process is incomplete. Measure comparable idle,
log-flood, build and multi-container workloads before claiming an advantage
over Docker Desktop. RSS can double-count shared pages and does not measure
all host VM/kernel overhead. Short-lived children between samples are omitted.

Local validation on 2026-11-16 passed the native VM certification, 619 workspace
tests on the CI-pinned Rust 1.99.0 toolchain, strict Clippy and advisory gates. The generated SBOM passed the official
CycloneDX 1.5 JSON schema and format checks. Settings was visually checked at
normal width and the 920 × 560 content minimum. Remote Linux/Windows CI,
release signing/notarization and a like-for-like Docker Desktop performance
comparison remain separate evidence; they were not performed by these checks.

Correction: the initial observation in `dist/verification/20261116/resources.json`
omitted Apple's separate `Virtualization.VirtualMachine` XPC process. Its
206.1 MiB figure measured only part of the stack and must not be used as
Hopper's engine footprint. Guest memory usage is a different accounting scope.
See `notes/replacement.md` and the replacement verification directory for the
corrected release-build measurement, including that VM process. Historical
artifacts are retained with a correction notice.
