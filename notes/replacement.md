# Replacement and resource verification

Settings now offers **Coexist or replace**. On macOS, filesystem discovery finds
Docker Desktop, OrbStack, Colima, Rancher Desktop, Podman Desktop and Podman,
including installed applications whose engine is stopped. Users can keep using
the detected engine, start Hopper instead while retaining it, import from that
specific source, or review removal. Discovery itself does not start or remove
other engines. Detection is evidence-based, not an exhaustive inventory of
custom installations.

Removal is an explicit Terminal handoff using a recognized Homebrew receipt or
Docker's signed vendor uninstaller. Unsupported installations get official
instructions. The review requires independent backups, verified replacement
workloads and the exact application name. Terminal asks for the name again and
checks Hopper's private Docker socket before executing. The Host rechecks the
installation, selected working Hopper VM and operation admission. Success is
not inferred from opening Terminal: the user must finish the vendor command
and refresh discovery. No installed user application was removed during
verification; execution tests use a fake uninstaller and isolated Unix socket.

Docker Desktop removal destroys its local container data; Hopper import does
not copy volume contents. See [Docker's uninstall documentation](https://docs.docker.com/desktop/uninstall/).
Other tools may leave machines or data behind; Hopper does not purge their
data directories or shared Docker configuration. CLI setup can preserve and
replace recognized dangling vendor links after removal, while retaining live
or custom links. Linux/Windows still need an installed compatible engine;
this change offers official manual guidance there rather than removing the
native engine Hopper requires.

Log streaming now coalesces UI updates at most ten times per second with a
bounded newest tail and waits without a periodic timer when quiet. A full
notification queue no longer terminates log streaming. Docker framing uses
BytesMut splitting rather than repeatedly shifting the remaining packet;
line parsing avoids unnecessary allocations and full-string boundary scans.

## Validation

On Apple silicon, 2026-11-16: 627 workspace tests passed, seven live/opt-in tests
were skipped by the default suite; strict Clippy and the optimized app build
passed on Rust 1.99.0. Normal-width Settings and removal review were checked
visually. Incorrect confirmation and unavailable replacement socket refused a
fake uninstall; correct confirmation with a ready fake socket executed only
the fake command. Actual vendor uninstall, remote Linux/Windows UI and release
signing/notarization were not exercised by these checks. Native managed-VM
certification from the preceding reliability work is recorded separately.

The release preview used an isolated root at `/tmp/hopperreplacecheck`, Docker
29.9.3, two CPUs, a 2 GiB guest, a sparse 60 GiB disk and no host folder grants.
The sampler explicitly includes Apple VM XPC PID 76338, started one second
after this VM's hostagent PID 76236. That service is outside Lima's process
tree; the earlier sampler omitted it and substantially underreported memory.
Historical measurements are retained with a correction notice.

These short local observations do not establish long-term stability, the
smallest workable guest allocation or superiority over Docker Desktop. RSS
can double-count shared pages and excludes some kernel overhead. CPU is a
process-time delta expressed as a percentage of one core. Guest usage is not
an additional independent host-memory total. The configured 2 GiB guest is
the dominant cost: smaller allocations require workload-specific validation.
Optional empty-engine suspension is available, off by default. Idle suspension
currently requires no containers, including stopped containers.

See `dist/verification/replacement20261116/summary.json` for corrected empty
and paused-workload observations and retained evidence. Further priorities are
multi-day sleep/wake and network-failure soak tests, representative build and
multi-container benchmarks against Docker Desktop, and measured evaluation of
smaller guest allocations and memory reclamation. No minimum-resource or
production-stability guarantee is established by the current suite.
