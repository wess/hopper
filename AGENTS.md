# Hopper

The isolated Windows production diagnostic now verifies complete official ESD acquisition,
checksum validation, ESD-to-ISO conversion, WinPE rendering and unattended deployment through
DISM, driver injection, recovery and UEFI boot setup. Initial installed startup failed: a disk-copy
boot renders Windows Recovery for msseccore.sys with 0xc000007b, then requests shutdown.
Read-only Alpine NTFS inspection finds zero bytes at that driver's header and a different
SHA-256 from the official Professional image; ntoskrnl.exe matches the image exactly.
Do not disable security checks or delete drivers to hide failures. The controller previously
force-stopped after the deployed marker without a guest filesystem shutdown acknowledgement.
The replacement handoff now retains Setup/Deployed as incomplete, acknowledges through a
private worker-only FinishDeployment command and fixed setup-port message, and lets WinPE
call wpeutil shutdown. Only an observed Shutdown plus worker reap permits durable Deployed
and system startup; requested stops, errors and the 120-second timeout require recovery.
The public session request API rejects the acknowledgement. Setup receipts now bind the
bootstrap script as well as the deployment plan, rejecting older media instead of silently
reusing its forced-stop behavior. Eight installation and nine setup checks pass, including
delayed shutdown, unclean shutdown preservation and public-command exclusion. All engine
tests pass (292 passed, three ignored); workspace all-target strict Clippy passes. A fresh
production cached-media installation reached deployment but failed the batch shutdown
acknowledgement and correctly remained Interrupted/Deployed without system startup.
A diskless signed WinPE probe confirms set /p returns immediately and closes the virtio
port; host acknowledgement therefore rejects a closed port. native/guest/finish.c now
implements a retained read/write overlapped port exchange with a total 60-second deadline,
bounded exact acknowledgement and cancellation. A second real probe receives the host
acknowledgement and completes wpeutil shutdown (PSCI Shutdown) in 14 seconds. The production
Windows asset builder now cross-compiles that freestanding ARM64 helper using LLVM, bundles
it at guest/hopperfinish.exe with the exact MIT project notice, and validates both in the
asset manifest and runtime admission. Setup media stages the helper and notice, binds their
SHA-256 values in its receipt, and rejects changed helper assets on reuse. Completion now
originates from the helper's retained port after deployment succeeds; the batch bootstrap
calls wpeutil shutdown only after helper success. The real build reproducibility check,
Python asset checks, related engine tests and workspace all-target strict Clippy pass.
CI now includes the real cross-build check; live workflow execution remains unverified.
A fresh production installation has now completed the helper acknowledgement, clean guest
shutdown and owned system-worker handoff. Independent extraction verifies the exact helper
and project notice in its real WinPE WIM against both source assets and receipt hashes.
Installed Windows now completes first-boot provisioning and renders a Windows 11 desktop
with the Start menu signed in as Hopper. Read-only post-stop NTFS inspection verifies both
msseccore.sys and ntoskrnl.exe exactly match the official Professional image SHA-256 values;
the previous all-zero driver is corrected. No security checks or drivers were disabled.
This proves automatic cached-media deployment, clean handoff, installed boot, sign-in and
native framebuffer rendering. Account group/privilege checks, dedicated viewer input,
Windows guest tools, networking and the remaining PRODUCT.md gates remain open.
Only the disposable failed retry disk was removed for space; its metadata and
captures and the original failed VM remain retained. Reproducible engine build artifacts
and obsolete probe staging were also cleaned; the successful probe ISO and frames remain.
The systemboot diagnostic uses the production device layout on an explicitly supplied disk
copy and in-memory firmware variables, preserving the source store. Stopped runtime results
retain only the existing bounded 64-byte UART tail for local diagnostics; no agent exposure
was added. Machine all-target strict Clippy passes. Windows desktop readiness remains open.

Fresh Ubuntu tool provisioning now enables and starts hopperguest.service after daemon
reload; enable alone did not activate it during the first boot. Provisioning now also
installs the verified helper notices at /usr/share/doc/hopperguest/licenses.html, root-owned
and mode 0644. The seed user-data bound is 8 MiB for the bounded executable and notices.
linuxtools (3), linuxseed (4) and real linuxassets (4) checks pass, including independent
ISO extraction of the exact built executable and notices; strict engine/MCP lint passes. The guestagent --ubuntu diagnostic uses the
production automatic acquisition, unattended installation, installation watch, authorized
installer retirement and installed-system admission, then the external MCP checks.
The first automatic acquisition/install/system-boot run completed deployment, but installed
MCP checks failed: the guest journal reported private boot input ended early. Read-only
rescue inspection verified the helper, udev rule and enabled systemd service were installed.
Private bootstrap delivery now waits for a fixed HBR1 request on the protected duplex port
before sending HGB1 material. The owner polls it on the VM queue with original checks,
a 180-second running-boot deadline and zeroized buffering; stopped/dropped ownership closes
it. Five host delivery tests and a signed real Alpine command/file diagnostic pass.
A fresh installation through the production verified-cache path now passes installed MCP
normal/root identities, binary create/read, browsing, streaming checksum/readback and
persisted policy revocation with process-tree cleanup. Read-only post-stop inspection
verifies the installed executable and complete notices match bundled SHA-256, size,
root ownership and 0755/0644 modes, and the journal confirms first-boot service start.
The two isolated runs establish official automatic acquisition, fresh cached installation,
system admission and installed agent operations. Existing guest-tool upgrades, GUI/viewer
integration and other OS release gates remain unfinished.
Guest licenses now include the complete official musl 1.2.5 COPYRIGHT with provenance and
recorded hashes in crates/guest/licenses. Generation requires reviewed Rust 1.99.0 and
rejects missing/altered notices. CI/release Rust setup is pinned to 1.99.0. Four generator
tests, package rebuild and four real asset checks pass; workflow YAML parses. This closes
the previously missing guest musl notice for that toolchain, not the full app license gate.

The six standalone Linux guest test harnesses now pass in a signed isolated Alpine VM
as both root and UID 1001. This executes the previously compile-only file, binary-name,
creation/CLI, upload/CLI, producer/backpressure/cancellation and bootstrap cases. CLI tests
accept HOPPER_GUEST_TEST_EXECUTABLE only in their test harness; no production command
contract changes. scripts/check/guesttests.py packages bounded stripped ARM64 harnesses
from Cargo JSON artifacts for machine's vzguesttests example. This is not Ubuntu or other
OS tool evidence.

The signed mcp example guestagent now verifies the real external route for an isolated
owned Alpine VM: production MCP stdio in a separate process with initialization, Host's
same-user socket, guest normal/root identity jobs, binary create/read, browsing and
streaming upload with checksum receipt and stamp-checked readback. Persisted access
off denies MCP; after re-enable, the old policy generation rejects, the old guest
process is gone and prior session job history is unavailable. This supersedes older
missing external MCP/revocation evidence for this diagnostic case only. Console output
is bounded to 1 MiB; teardown kills the child and stops/retires hardware, and failed
runs retain evidence. Strict MCP all-target lint passes. It uses an isolated synthetic
record and Alpine hardware, not installed Ubuntu or the GUI, and calls the production
MCP server loop without the shipping binary's container-engine-selection startup.
Installed service activation/update, inside-guest agents and full release gates remain open.

Latest evidence, 2026-10-12: an unrestricted signed vzcommands diagnostic now passes on
real VZ hardware with the production helper and checksum-verified official Alpine 3.24.2
ARM64 media. This supersedes earlier virtualization-unavailable diagnostic status. It
covers private boot delivery and normal-user key/device denial, UID 1001/root jobs,
small-file binary create/read/preservation, streaming uploads/readback, fully queued
uncommitted cancellation and later-commit rejection, checksum rejection without a target,
directory pagination, protected-directory/pseudo-filesystem denial, cancellation and
stopped/runtime ownership cleanup. It does not establish installed Ubuntu activation,
external MCP-to-VM round trips, persisted-policy revocation or other OS guest tools.
Named-port discovery skips unnamed console ports; other attribute errors still reject.
The guest daemon verifies actual local CID through the Linux vsock ioctl because an
auto-bound socket may report CID_ANY; host endpoints and non-host peers still reject.
Failed command probes retain bounded console evidence. Socket tests previously blocked
by EPERM now pass: remotejobs (2), control (5), engine machines (9), MCP machines (1).

Native Linux vm.file_browse routes literal selected-role helper jobs through the original
authenticated policy-bound channel. Requests bound pages to 32 entries and raw cursors to
255 bytes. Retained-descriptor enumeration holds at most 33 candidates, bounds scans to
65,536 real entries, sorts raw byte names and never follows entry symlinks. Directory
identity/time stamps reject observed changes; unknown entry types remain unknown and
listing is not an atomic snapshot. Linux proc/sysfs browsing rejects. A successful
untruncated job's stdout contains Listing JSON with stamp, entries and next cursor.
Three host browse tests, five file-contract tests, two MCP tests and four package checks
pass, with strict musl lint. Linux-only standalone tests remain unexecuted. Other OS
browsing, installed service updates and the full PRODUCT.md release gates remain open.

The Linux guest-helper package requires checksummed `licenses.html` alongside its executable
and systemd unit. `scripts/build/guestlicenses.py` collects target normal/build dependency
notices and the matching toolchain standard-library copyright document; install `rust-docs`
when building. This partial inventory does not establish the full app redistribution gate:
separately linked musl and all other bundled components still need audit. The declared MIT
project license text is in `LICENSE` and must also ship in app resources. Release build jobs
install the musl target, rust-docs, LLVM/lld and ACPI tools; bundle verification checks guest
assets and the matching project license. Updated release automation remains unverified live.

Native Linux `vm.file_read` maps a bounded absolute guest path/range and explicit execution
role to a literal `/usr/libexec/hopperguest --read-file` job, through the original policy-bound
job channel. Read jobs return raw stdout bytes and a JSON stderr receipt; only successful,
untruncated completion is usable. A caller can reuse the observed inode/size/time stamp for
later chunks. Linux pins with O_PATH and validates regular type/filesystem before opening
for data, preserving worker identity permissions and rejecting proc/sysfs and symlink leaves.
Chunks are at most 64 KiB, files at most 1 TiB and jobs have a 30-second timeout. Metadata
checks do not provide an atomic snapshot. Host reader/contract/MCP tests and host/musl Clippy
pass; actual guest CLI, Linux permission/special-file checks and full transfer remain unverified.
No host file reader is connected. Large/resumable uploads, browsing and other OS transfers remain pending.

Native Linux `vm.file_create` now accepts a typed bounded 32 KiB byte payload and explicit
role. The guest daemon supplies a private mode-0400 sealed memfd as job stdin; ordinary jobs
retain null stdin, and payload bytes never enter argv. After identity reduction, the fixed
guest helper writes a mode-0600 O_TMPFILE inode, syncs data and publishes through the pinned
directory with linkat without replacement. Existing destinations reject; unsupported filesystems
fail. Failure/cancellation after publication may leave a complete destination and never authorizes
replacement. Poll for successful untruncated completion and parse the JSON stdout receipt.
Contract/MCP/framing/package checks and host/musl Clippy pass. Linux-only syscall/CLI tests
compile but remain unrun; the new Linux CI job is also unverified. The updated signed queue
diagnostic checks normal/admin round trips and preservation, but its live attempt still fails
with virtualization unavailable before those checks. Large/resumable uploads, replacement,
browsing and Windows/macOS transfer remain pending. This does not close the release gates.

Repository guidance for agent sessions.

## Desktop virtual machines

`machine::hypervisor::gic::state` captures bounded SDK-versioned interrupt-controller
bytes while every CPU is held. Restore requires a recreated GIC and all CPUs, before
any CPU runs; CPU interface registers are separate. The signed `interruptstate` probe
verifies wired and MSI pending interrupts across VM destruction/recreation, bounded
decoding and invalid-state rejection without losing those pending interrupts. This is
one component of running snapshots, not a usable snapshot endpoint. CPU/RAM/device/disk
capture, recovery publication and full open-application restoration remain pending.

`machine::hypervisor::state` now captures/restores owner-thread general, FP/SIMD,
core EL1/EL0 system, CPU interrupt-interface, timer offset/mask and pending SError
state. A small Clang shim preserves the SDK SIMD vector ABI. Captured buffers erase
on drop; they are not agent-visible. The signed `cpustate` probe recreates the VM,
checks register equality and resumes real guest instructions with nonzero thread,
stack, FP and all 32 SIMD values. The standalone record excludes PMU; coordinated
runtime capture preserves the exposed cycle-only PMU separately. Wider vectors and
complete durable VM serialization remain pending. Restore failure requires discarding the VM;
never resume partially restored CPUs. This component does not enable snapshots.

`hypervisor::state::wire` now encodes architectural CPU state as HCS1: bounded 4 KiB,
little-endian counts/explicit bank identities, header/payload SHA-256, fixed general and
SIMD dimensions, feature-counted debug entries, timer offset/mask and SError. Decoding
checks integrity before allocating owned register buffers and rejects unknown register
identities, dimensions/flags, extra/truncated data and unsupported SVE/SME. Owned encoded
and temporary register buffers erase on drop; no Debug or raw serde API exposes state.
The checksum is not authorization or VM binding. Five pure checks cover full/maximal
banks, every truncation/one-bit damage, rechecksummed schema edits and incomplete state.
The signed `cpustate` example now supports private-file --save/--restore modes.
`python3 scripts/check/cpustate.py` proves separate-process reconstruction and real guest
continuation with nonzero registers/SIMD/debug, plus symlink, hardlink, FIFO, directory,
public/oversized file, public parent, checksum and overwrite rejection without changing
the original bytes/inode/mtime. Files and directory are explicitly mode 0600/0700 and
synced; reads open no-follow/nonblocking before checking regular type/owner/link count
and bounded length. The directory helper is diagnostic-only, not a pinned production
publisher. Full machine tests and strict all-target Clippy pass.
RAM/device/disk serialization, bound manifests, atomic publication/recovery and complete
durable VM snapshots remain pending; no worker/agent snapshot endpoint is enabled.

`hypervisor::state::group::wire` now encodes HCP1 within 8 KiB, bundling HCS1, the frozen
guest counter and HPM1 PMU state. HPM1 has eight explicit register identities and binds
the cycle-only bank to CPU affinity, exact PMU features and guest counter frequency.
Decode rejects incompatible normalized host timebase units before hardware allocation;
cross-component validation checks affinity/features and frequency against host tick units.
Checksums are corruption detection, not authentication or VM binding. Four combined codec
checks cover round trips, truncation/bit damage, rechecksummed foreign metadata and mixed
component encoding. The signed `pmurecovery` diagnostic syncs private CPU records, destroys
their original in-memory states, reopens the bounded files and reconstructs hardware twice
with distinct PMU epochs/settings and held-then-expiring physical/virtual timers.
Remaining devices and storage still remain in process: this is not a process-durable VM snapshot.
The signed PMU diagnostic rejects foreign affinity without changing destination context.
The production restore checks affinity, exact PMU features, zero event-bank count and
counter frequency before changing PMU registers. Full snapshot durability and endpoints
remain pending.

`hypervisor::gic::state::wire` now encodes HGI1: bounded 16 MiB opaque SDK payload,
explicit length and header/payload SHA-256. State and encoded buffers erase on drop.
Decode verifies framing/integrity before allocating owned state; opaque compatibility
still belongs to the SDK during restore. This does not validate guest topology or
authenticate/authorize a snapshot. Two pure codec checks cover payload bounds and
round trips, truncation/damage/extensions and rechecksummed wrong lengths. The signed
`python3 scripts/check/gicstate.py` check saves/exits and restores in a separate process,
verifying enabled SPI/MSI, pending bits and priorities, followed by MSI clear/re-signal.
It also verifies private-file/type/checksum/overwrite rejection and original preservation.
No guest interrupt handler is run by that component check. `pmurecovery` now destroys
the captured in-memory GIC state and reopens its synced private record before two-CPU
repeated reconstruction. Directory helpers remain diagnostic-only; durable RAM/devices/
storage, VM-bound manifests, production publication/recovery and endpoints remain pending.

`hypervisor::ram::wire` now streams HRM1 banks with a guest-address/logical-size/chunk-size
header, 64 KiB chunks (zero chunks omitted) and a complete stored framing/data SHA-256.
Banks are at most 64 GiB, host-page aligned and within the 40-bit address bound. Decode
requires caller-authorized bank address/size and rejects mismatches before allocation;
it streams into private anonymous memory, then reduces maximum protection to read-only.
Temporary data buffers erase on drop. Authorization is checked between chunks and before
success; blocking I/O still requires caller bounds. Any encode error forbids publication,
including a final-policy failure after the complete file was written. Checksums do not
authenticate the state. Three pure checks cover sparse/partial round trips, read-only
maximum protection, bank/schema/damage/trailing rejection, revocation and failed output.
`python3 scripts/check/ramstate.py` verifies signed separate-process restoration and real
guest execution from a sparse 32 MiB bank, repeated COW recovery and guest/host write
isolation, plus unsafe-file/checksum/overwrite rejection. `pmurecovery` now syncs firmware,
NVRAM and RAM records and destroys all original memory captures before reopening them,
alongside CPU/PMU/clock/GIC records, then reconstructs and continues both CPUs twice.
Remaining device/storage state remains in process. Maximum populated-bank throughput and
pressure, durable device/disk state, manifests, production atomic publication/
recovery, full VM process restoration and worker/agent endpoints remain pending.

`snapshot::wire` now encodes HSW1 software/actions records within 512 bytes: current native
one/two-CPU affinities, online/parked state, debug locks, up to sixteen ordered executable
regions, bounded UART tail and relative boot-key nanoseconds (at most one second).
Strict topology/count/boolean/primary-online/action checks run on encode/decode. Restoring
software still validates exact authorized topology before mutation. `serial::wire` encodes
HUR1 PL011 registers and up to sixteen queued receive bytes within 69 bytes; decode checks
register masks and exact length. Do not restrict saved RX length to the current FIFO mode:
turning FIFO off does not discard already queued bytes. Both formats carry SHA-256 for
corruption detection, not VM authentication. Five pure tests cover round trips, parked/debug
and relative action behavior, FIFO transition, truncation/damage, rechecksummed invalid
state and unsupported/foreign topology rejection. Signed group/recovery checks now sync
these private records and destroy original software/action/UART objects before reopening,
alongside CPU/PMU/clock/GIC and memory records. The parked CPU remains off until actual
guest CPU_ON; the delayed boot key survives a 1.2-second hold and delivers once after
resume; guest-written UART registers and subsequent transmission survive reopening.
Remaining device state and storage still remain in process. Full durable runtime
reconstruction, manifests, production atomic publication/recovery and endpoints remain pending.

`flash::snapshot::wire` now encodes HFL1 command/protection state within 263,000 bytes:
bank/block geometry, exact block-lock booleans, mode/status and explicit pending-command
tags with bounded base/remaining count and up to 32 ordered words. Decode rejects invalid
geometry, modes/status, noncanonical fields, unaligned/out-of-bank blocks/words, duplicate
addresses and buffered writes crossing their 128-byte window. Restore also validates the
command state before allocating a fresh flash bank. NVRAM contents remain a separate HRM1
record. Encoded bytes erase on drop; SHA-256 is corruption detection, not authentication.
Existing flash traces now destroy/reopen the captured state at every command boundary
before continuation. Three additional codec checks cover bounds/full buffers, damage,
truncation/extensions and rechecksummed unsafe metadata. Signed group/PMU recovery now
syncs flash records and destroys original flash state before reopening. The live pending
write fixture recreates hardware and resumes real guest instructions to complete its
buffered write/confirmation, checking final NVRAM and unchanged original captured bytes.
Virtio/PCI backend state, storage/media captures, manifests, production atomic publication/
recovery, complete VM process restoration and worker/agent endpoints remain pending.

`virtio::pci::snapshot::wire` now encodes HVT1 transport metadata within 8 KiB, using
explicit stable backend-kind tags instead of serializing Rust discriminants. Records
include offered/driver features, common/queue/window registers, quiescent ring positions,
ISR, completion count, bounded UTF-8 fault text and nested PCI/MSI-X state. HPC1 PCI
decode requires a matching authorized Function layout, checks capability/BAR geometry
and only permits configuration reachable through existing writable register paths;
immutable/reserved changes, bad BAR alignment and invalid probe halves reject. HQR1
queue decode checks sizes/alignment/overflow/overlap and uninitialized progress; actual
restored RAM remains checked by queue restore before mutation. HMX1 bounds table count
to 64 and validates control/pending bits. All encoded buffers erase on drop and checksums
detect corruption without authentication. Pure checks cover damage/truncation, invalid
rechecksummed metadata, fresh-layout BAR/probe recovery, 64-entry pending MSI-X delivery,
queue wraparound without replay and unchanged RAM on failed restore. The signed group
diagnostic now syncs/reopens each transport record before runtime reconstruction and
recapture. Transport reopen consumes original metadata but retains its backend and
matching layout; it is not a complete process-durable device record. Manifests, production publication/
recovery, complete VM process restoration and worker/agent endpoints remain pending.

`input::wire` now encodes HIN1 within 9 KiB: device kind, selectors, LEDs, up to 1024
pending events and ordered unique delivered held keys/buttons. Decode validates internal
normalized tablet axes (0..32767) directly; do not feed reopened events through enqueue
and normalize twice. `console::wire` encodes HVC1 within 134,000 bytes: bounded name,
negotiation flags, up to 32 canonical controls and two 64 KiB byte buffers. Decode checks
counts, boolean/state relations and name-bound control packets. Both formats checksum
stored bytes and erase encoded buffers; checksums are not snapshot authentication.
`snapshot::backendwire` supports only these components, destroys original contents before
decode and retains only input-kind/serial-name binding. Foreign bindings reject. It does
not restore host input leases or authenticated command sessions. Pure checks verify held
release vs unsent presses, LED/selectors, normalized axes, maximum queues/full buffers,
negotiation controls, corruption/schema rejection and binding mismatch. Signed group
recovery syncs/reopens input backends before device use, recapture and delayed-key delivery.
Console queue simulation passes; installed guest serial exchange after reopening remains
unverified. Manifests, production atomic publication/recovery,
complete VM process restoration and worker/agent endpoints remain pending.

`gpu::snapshot::wire` streams HGF1 with a 400 MiB upper bound: resource pixels and
backing spans, scanout/published frame, cursor and independent active linear layout/frame.
Pending linear registers may differ from the active layout and must remain independent.
Preferred display dimensions bind before allocation; resource count is at most 64,
aggregate resource pixels at most 128 MiB, each backing list at most 4096 spans and
each frame at most 128 MiB. Strict booleans, ordered unique IDs, supported pixel formats,
geometry/state relations and SHA-256 corruption checks reject malformed records.
Restore still validates matching guest RAM. Streaming checks authorization around at
most 64 KiB I/O chunks; callers must bound blocking I/O and discard failed output.
`pci::snapshot::gpuwire` drops original GPU contents before decode, retaining preferred
dimensions only. Signed group diagnostics sync/reopen private GPU files before device
checks, execution and recapture. Pure tests cover published/unflushed pixels, cursor,
active/pending linear state, malformed metadata, revocation, failed writes and chunk
bounds. One combined group run failed at a PMU checkpoint; the diagnostic rerun and
standalone two-CPU PMU recovery passed.
The failure remains unexplained; PMU errors now include register/expected and actual PC.
No claim of full process restoration, production publication, Windows open-app recovery
or agent snapshot integration follows from these checks.

`snapshot::storage::record` stages immutable APFS image clones as a fixed `image` name
under a caller-owned private directory descriptor. HDS1 is exactly 76 bytes: magic,
logical capacity, complete SHA-256 image digest and receipt corruption checksum. Save
refuses replacement and syncs the image/directory before returning. Load validates
trusted expected capacity (1 byte to 2 TiB) before cloning, requires a regular single-link
same-user mode-0400 image and mode-0700 directory, then clones into fresh private ownership
before hashing. Source paths are never retained. Reads hash at most 64 KiB per chunk,
including holes, with authorization checks around chunks and clone/sync boundaries.
Hashing can be expensive and must remain outside the brief capture pause; OS calls are
not deadline bounded. Failed stages must not publish, including cancellation after sync.
No manifest transaction, crash recovery or garbage collection is implemented here.

`block::snapshot::record` adds fixed 142-byte HDB1 metadata: capacity, read-only/writeback
flags, ASCII controller identity, HDS1 and checksum. Expected controller binding rejects
before image access; read-only writeback rejects. `pci::snapshot::storagewire` drops the
original disk state before reopening and retains only that binding. Private diagnostic
checkpoint files now include disk records. Separate-process tests terminate the saver,
then recover guest-written sectors in two fresh processes and verify independent writes.
Storage tests cover sparse independent forks, unsafe files, checksum damage, path pinning,
capacity/binding mismatch and revocation at every save/load authorization checkpoint.
Signed coordinated recovery verifies reopened disk sectors and recapture. Full process
restore/rollback/open applications, production publication/recovery
and worker/agent snapshot endpoints remain pending.

`scsi::snapshot::record` adds fixed 2,270-byte HDO1 metadata and HDS1 image receipt:
capacity, negotiated sense/CDB lengths, canonical pending sense, insertion state, all
256 command counters, error/queue counters and optional missing-LUN/control/transfer
diagnostics. Booleans and unused optional payloads are canonical; event queue counts
remain zero, notification kinds cannot appear as retained controls, and retained
bidirectional transfers stay within 1 MiB per direction. Capacity/checksum/schema
validation precedes media access. Reopen drops original optical state before verified
independent image loading. `pci::snapshot::storagewire` handles both disk and optical
records; private diagnostic files use `.hvs` with the typed magic inside. Both controller
save paths recheck authorization after final metadata encoding.
Separate-process tests terminate the saver, then recover ejection, pending sense and
captured sectors in two fresh processes despite different target contents. Existing
optical tests now reopen records before checking all statistics, negotiation, repeated
independent restores, read-only behavior and source/path replacement. Bounds checks cover
minimum/maximum negotiated lengths, damage/truncation/extensions, rechecksummed invalid
state and final authorization. Signed group recovery now syncs/reopens both optical
records before device checks, execution and recapture. Full VM process restoration,
atomic manifests/publication, crash recovery, rollback/open applications and worker/agent
snapshot integration remain pending.

`pci::snapshot::wire::decode` now validates HVT1 against a fresh owned `Device`, without
capturing or retaining its backend. It returns transport-only state with no backend.
The existing `reopen` wrapper remains available for component tests. `snapshot::records`
reconstructs a complete device from typed small-byte, streaming graphics or pinned-image
contents and fresh authorized hardware. Input kind, serial name, display dimensions and
disk/media bindings derive from that hardware; target contents are never captured as
snapshot contents. Foreign types/bindings reject and authorization checks bracket decode
and completion. A transport-only state cannot pass complete backend restoration.
Restoration still validates queues/backings against the matching guest RAM before mutation.
Diagnostic checkpoint reopen now destroys all original CPU/GIC/memory/software/device
objects before invoking a fresh hardware factory and checks exact device-slot presence.
Signed group checks cover device use, full runtime execution, recapture, parked CPU and
pending actions after this change. Separate-process input-device tests terminate the
saver and twice recover pending events, delivered held-key cleanup, BARs, queue progress
and ISR state. Existing serial/GPU/disk/optical tests also decode against fresh hardware.
Complete VM process restoration, manifests/atomic publication/crash recovery, rollback/
open applications and worker/agent snapshot integration remain pending.

`snapshot::manifest` now encodes HSM2 within 1,200 bytes (maximum inventory is 1,165):
caller-supplied machine/snapshot IDs and configuration fingerprint, 1–2 CPUs, three
nonoverlapping bounded memory banks, seven stable device-kind slots and exactly the
required ordered component list. At most 24 entries bind component length and existing
SHA-256 payload footer. Missing/duplicate/unordered/unknown entries, foreign identity/
configuration/topology, invalid geometry and format-specific upper bounds reject.
Record names derive from typed roles; diagnostic backend files now uniformly use
`backend{slot}.hvb`, with typed magic inside. Raw disk/media images are linked through
their HDS1 receipts, not treated as footer-bearing component files.

`Manifest::reader` caps reads at 64 KiB, checks authorization before/after I/O, permanently
fails after errors and zeroes newly read bytes on post-read revocation. Its consuming
`finish` checks EOF, exact consumed length and the rolling consumed footer, then checks
authorization again. Every component decoder must succeed too: footer admission does
not verify payload integrity or authenticate the manifest. This avoids duplicate full
memory hashing while preventing substitution of another valid component after opening.
Do not infer a usable checkpoint from manifest construction or footer checks alone.
Tests cover full inventories, all format bounds, truncation/byte damage, rechecksummed
invalid fields, foreign bindings, fragmented footers, valid same-length substitution,
bounded large reads and revocation through final admission.

Signed diagnostic reconstruction syncs/reopens a manifest after destroying original
objects, uses explicitly synthetic fixture identity and checks all decoded components
before runtime execution and recapture. One attempt failed before manifest loading with
an unexpectedly stopped secondary CPU; its diagnostic rerun passed. Native runtime now
joins already-finished secondary owners to expose their actual error instead of masking
it behind that generic message. The intermittent failure remains unexplained. Production
VM identity/configuration integration, worker package publication/crash recovery,
complete VM process restore, rollback/open applications and worker/agent endpoints remain
pending; the manifest codec does not close those release gates.

PMU register access now restarts the intended fixed code slot and write operand on each
canceled retry, retaining the 16-attempt bound and 100 ms context watchdog. Previously,
cancellation reported at a completed HVC could continue into the adjacent access stub.
The deterministic signed injection reproduced PMCR read checkpoint PC 0x3fff0010 instead
of 0x3fff0008 before the fix. Its regression now checks read retry, write operand replay,
16 repeated cancellations and unchanged original architectural context. The runner hook
is private and does not expose instruction selection to agents. `scripts/check/pmu.py`
also runs a live 1,024-capture diagnostic with a concurrent CPU-exit requester, checking
PMU controls and CPU context through successful captures or bounded interruption errors.
Six signed group runs passed before this fix without reproducing the natural failure;
fault injection establishes the retry defect, not causation of the earlier intermittent
PMU/secondary-owner errors. That relationship remains unproven. Cargo can rebuild example
executables without their test entitlement; re-sign after building, and use private
signed copies for repeated runs alongside builds. No release gate closes from stress
passes alone. Three post-fix signed group runs, standalone two-CPU PMU recovery,
full machine tests and strict all-target Clippy passed.

A later natural full-checkpoint run reproduced adjacent-slot execution despite that
retry reset: PMCCNTR read expected PC 0x3fff0058, observed HVC0 at 0x3fff0060.
The preserved failed fixture is `/var/folders/__/c5cv93yj28g33k_s3ccyk3_00000gn/T/hoppervmstate.yc2qb134`.
Each read/write stub now occupies an isolated 16-byte slot: register access, HVC0,
branch back to its own access, then an HVC1 guard. A missed checkpoint cannot fall
straight into a neighboring register operation. Deterministic continuation after HVC
reproduces the old adjacent-slot error and now verifies both reads and writes, retained
PMU controls and complete architectural-context restoration. This establishes containment
of that execution path; the underlying reason the natural checkpoint was missed remains
unproven. Retry/deadline bounds are unchanged. Production snapshot integration and the
other release gates remain open. Eleven full fresh-process diagnostic runs pass after
isolation, each including two restores, independent recapture, authorization rejection,
unchanged source and duplicate/substitution/damage rejection. Signed fault/timer tests,
1,024 interrupted captures, retry/fallthrough injections, full machine tests and strict
all-target lint pass. These passes do not prove the natural missed-exit cause resolved.

The isolated-stub build also passes installed Windows fresh-process recovery in
`/tmp/hopperrecovery119.process.ytij8ckr`: current-date lock screen, identical held
framebuffer, then Hopper sign-in after guest Space input. Disk requests progress from
17,203 to 27,417 and keyboard requests to four without device faults. Serialization takes
51.31 seconds, decode 43.13 seconds, hardware reconstruction 44.98 milliseconds and the
resumed guest runs 30 seconds. Snapshot metadata, its disposable source and the original
installed guest remain unchanged; `evidence.json` records visual review and frame hashes.
This is sign-in/input evidence, not open-application preservation or production integration.

The native snapshot package module stages under a pinned private directory descriptor,
uses typed component names and exclusive atomic directory rename, and never replaces
an existing snapshot identity. The writer checks full payload SHA-256 footers while
streaming at most 64 KiB per write, enforces topology-derived bounds, syncs mode-0400
files and poisons the stage after encoder or write failure. Publication requires every
manifest entry and syncs manifest/stage/parent. A failure after rename may leave a
complete package: inspect the same identity, never replace it. Abandoned stages are
preserved; ownership-aware crash recovery and GC remain pending. Package open checks
private directories, identity/configuration/layout and bounded files; every component
must still pass its decoder and consumed-reader finish before use. Image receipts remain
separately verified by storage loaders. Trusted image encoders receive a pinned directory,
never an agent-selected path. Synthetic test records prove package mechanics only.
Separate-process package tests verify writer exit/reopen and no replacement; tests also
cover failed/truncated/corrupt/overbound writers, fragmented large writes, every write
and publication authorization checkpoint, unsafe files, replaced stages and pinned-parent
replacement isolation. Signed group recovery uses this publisher for actual component
records and reopens after original object destruction against fresh hardware. Fixture
identity remains synthetic. Production configuration binding, worker/agent integration,
installed-desktop process restore, rollback/open-app recovery and clones remain pending.
The machine library now owns full native checkpoint save/load. Save borrows immutable
captured COW state, runs after releasing the brief capture pause and supplies the original
authorization to image encoders during hashing. Load checks expected fresh hardware kinds,
decodes all components through manifest-bound readers and performs final authorization
before returning a checkpoint; no hardware is allocated or started by the loader. The
shared diagnostic wrapper now uses these library paths rather than its own serializer.

`scripts/check/vmstate.py` builds/signs a private executable copy, saves a complete running
two-CPU fixture checkpoint, waits for source-process exit and restores twice in separate
new processes. Both resume CPU loops and UART traps, verify graphics/keyboard/disk/media
state and recapture a running VM. The saved package retains hashes, inodes, sizes, modes,
link counts and mtimes. Duplicate publication, valid same-length CPU substitution,
checksum damage and revocation at open/load/final admission reject. The successful fixture
has 31,250 authorization checks; final-admission revocation uses the observed count rather
than assuming a fixed count. Full machine tests and strict all-target Clippy pass. This
is a minimal test guest with synthetic binding, not installed Windows/Linux/macOS or
open-app recovery. Production VM configuration binding, original-policy worker/agent
integration, crash recovery/GC, rollback and snapshot/clone endpoints remain unfinished.

The signed installed Windows diagnostic now supports separate save/load processes via
complete native checkpoint packages. `scripts/check/windowsstate.py` clones a private
disposable fixture with APFS, locks its source while cloning, signs a private executable
copy and waits for the save process to exit before load. Expected CPU/RAM/device layout,
firmware/disk-id/original-variable content and diagnostic display/counter metadata bind
admission; VM/snapshot IDs remain explicitly synthetic. The clean live fixture at
`/tmp/hopperrecovery119.process.s99bcbzs` has a bit-identical captured/held 1024x768 display,
Windows lock-screen-to-sign-in input verified visually, disk requests increasing from
15,260 to 15,457 and no device faults after thirty seconds. Package file inode/size/mtime
and both disposable/original-source metadata remain unchanged. Serialization took 52 s,
decode 43 s, and hardware reconstruction 46 ms. This does not measure or bound the brief
capture pause independently. Open applications and Windows-specific PMU behavior remain
unverified. The guest wall clock in that run was incorrect; the native clock fix below supersedes it.
The first fixture reached Windows recovery, not the desktop; its serializer was canceled
and its unpublished stage retained at `/tmp/hopperrecovery119.process.umoq_e4z`. Do not count
that first attempt as desktop evidence or reuse repeatedly force-stopped disks blindly.

macOS ARM builds enable the pinned sha2 0.10.9 `asm` feature (sha2-asm 0.6.4), selecting its
runtime-detected ARM SHA path with software fallback. Standard empty/abc/million-a vectors
and fragmented updates pass, as do full machine tests and strict all-target lint. The
new dependency's exact MIT notice is retained at `assets/licenses/sha2asm.txt`, copied to
app `Resources/licenses` and checked by the bundle verifier. Syntax and upstream notice
matching are verified; full signed bundle verification and broader redistribution audit
remain pending. The Linux guest target dependency graph excludes this macOS-only feature.
Production VM configuration/policy ownership integration, agent endpoints, installed
Linux/macOS recovery, open apps, rollback, clones and crash recovery/GC remain unfinished.

Native Windows cold boots now expose a PL031 RTC at 0x09010000, SPI 34, and a firmware
FDT node. The existing firmware driver previously found no clock node and could not
read time. It requires byte reads of the eight identity registers in addition to 32-bit
register accesses; the first live attempt caught that missing case and was corrected.
The RTC starts from host UTC with per-guest signed offset, loaded/frozen counter,
match/enable/mask/pending state and a last-sampled counter. Guest writes affect no host
clock. It continues with host wall time while CPU timers are held, unless guest-disabled;
host-clock reversal does not synthesize a crossed alarm. The supplied firmware owns it
through UEFI and disables its FDT node for OS-driver attachment. Wakeup services remain
whatever the existing firmware driver supports; do not claim OS alarm/wakeup QA.

Clock state persists in exact 61-byte HRT1 records and is mandatory in HSM2 manifests
(up to 24 components, 1,165 bytes). Previous HSM1 inventories lack clock state: reject
with explicit migration guidance and preserve them, never fill in a fresh clock silently.
Tests cover real prior HSM1 inventory rejection, complete new inventories, offset/wrap,
byte identity admission, MMIO bounds, control reset, alarm mask/clear, reversed host time,
disabled/active record reopening and recomputed malformed fields. Signed guest code now
reads RTC and sets a one-hour guest-only offset; captured and fresh-process loaded state
must retain it independently of host UTC. One repeated fresh-process restore failed,
but the old runner discarded its captured stderr and deleted the fixture. Its cause is
unknown. The runner now prints captured failures and preserves failed fixtures. The
immediate diagnostic rerun and three further complete runs (two fresh restores each)
passed; that does not establish resolution of the intermittent PMU/secondary/recovery
errors. Full machine tests, strict all-target lint and signed PMU diagnostics pass.

The installed Windows RTC fixture `/tmp/hopperrecovery119.process.3md13fgb` verifies a
current October 12 lock screen, identical held pixels after source-process exit and
fresh-process load, guest Space input reaching sign-in, disk requests 18,294 to 27,426,
no device faults and unchanged snapshot/source metadata. Serialization took 53 s,
decode 45 s, hardware reconstruction 64 ms and recovered execution thirty seconds.
Original installed source metadata also remains preserved. Open apps, production snapshot
identity/policy/worker/agent integration, rollback, clones and crash recovery/GC remain
pending, along with the full PRODUCT.md release gates.

CPU capture now also preserves feature-counted hardware breakpoint/watchpoint banks,
MDCCINT and MDSCR. A 12-register feature fingerprint is checked before any restore
mutation; SVE/SME-advertising configurations explicitly reject until their wider state
is covered. The signed probe verifies a nonzero guest-written debug address after VM
recreation and rejects a different PMU feature configuration without changing X0.
Temporary capture buffers erase on failure too. Signed SDK checks with the production
PMU feature enabled find tested PMU system-register reads return HV_BAD_ARGUMENT;
performance-counter preservation still needs a different implementation. Full running
snapshot integration remains pending.

The signed `pmu` diagnostic uses immutable, read-only guest code under the production
PMU feature configuration. On the current host it observes PMCR=0x40, zero event
counters, accessible PMU controls and a writable 64-bit cycle counter. The cycle
counter continues advancing during a 100 ms owner hold even after counter/interrupt
disable writes; it cannot be treated as frozen CPU memory. Guest-only architectural
access succeeds where the SDK register API rejects. This does not enable snapshots:
coordinated access, clock rebasing,
safe state restoration and cross-host capability checks still need implementation.

The expanded signed probe verifies PMU isolation between two CPU-owner threads and
reconstructs a CPU after architectural capture with seven nonzero PMU settings. All
seven reset on the current host: PMCR, PMCNTENSET, PMSELR, PMUSERENR, PMINTENSET,
PMCCFILTR and PMOVSSET; the seeded cycle value resets too. A 2^40-tick architectural
timer-offset change leaves the PMU cycle counter's epoch unchanged. An independent
signed SDK probe verifies PMU get_sys_reg still rejects and debug-register trapping
does not trap PMU accesses. Existing frozen timer restoration cannot cover PMU state.
The expanded example and strict all-target Clippy pass. Runtime PMU integration and
snapshot endpoints remain pending.

`hypervisor::pmu` now provides a separate held-owner cycle-only PMU primitive, using
fixed MRS/MSR/HVC instructions in private host/guest read-only memory. Tokens borrow
the mapping and can cross owner threads; a compile-fail check prevents lifetime escape.
Temporary execution masks exceptions, disables guest stage-one translation/debug and
installs private fault vectors. Cleanup restores the complete architectural checkpoint
plus original debug-trap configuration: a smaller save set missed first-entry SDK
changes on incompatible CPU rejection. Execution has a 100 ms exit deadline; failed
cleanup or partial PMU restoration requires VM disposal. Unsupported PMU versions or
nonzero event-counter banks reject. The signed probe verifies all seven settings and
the cycle epoch after full VM destruction, MMU-enabled context isolation, unchanged GIC
bytes/RAM sentinel, occupied-address preservation and incompatible-feature cleanup.
Full machine tests and strict all-target Clippy pass. Coordinated runtime
capture/final resume rebasing, installed Windows checks, durable
publication and worker/agent endpoints remain pending; snapshots are still disabled.

The real PMU fault/timer test is in `tests/pmu/probe.rs`, included as an external private
unit-test module so it can exercise the context without exposing an arbitrary-code API.
Ordinary Cargo runs ignore this entitled SDK test. `python3 scripts/check/pmu.py` discovers
the exact Cargo library-test artifact, signs/verifies it and runs only that test. Live
checks prove an undefined instruction reaches the private HVC vector without executing
a RAM-writing guest handler, and an infinite loop terminates through the 100 ms exit
deadline. Both retain architectural/GIC/RAM state. Routed timer tests preserve future
and already-due enabled physical/virtual controls, timer-delivery masking and a pending
physical interrupt through PMU capture/restore. The extended test begins with guest IRQs
enabled and crosses a live timer deadline inside temporary execution. Guest registers,
compare values, writable controls and original IRQ enable survive; the due physical
interrupt remains pending without entering the RAM-writing guest IRQ handler.
Installed Windows PMU verification remains pending.

Primary capture now runs before `Group::arm` publishes the shared frozen epoch. Arming
requires successful primary state; secondary/primary CPU capture waits for that epoch.
The primary's potentially blocking disk clones precede RAM/GIC capture, so a frozen epoch
can no longer precede slow primary storage work and later capture an inconsistent timer
interrupt cut. A signed private phase probe delays primary capture, rejects early CPU
collection, verifies the frozen host epoch follows the completed cut and rejects primary
failure. Full signed group reconstruction/capture regressions pass. This ordering is a
prerequisite for coordinated PMU capture/restore.

Coordinated `group::Captured` now includes a nonoptional cycle-only PMU state. Captured
and reconstructed runtimes retain a private code mapping outside guest RAM for their
entire CPU-owner scope; ordinary uncaptured boot has no new mapping. CPU collection
captures architectural state first, then PMU state on that owner after the primary cut.
PMU failure propagates fatally through runtime teardown rather than leaving an owner
eligible to resume after failed cleanup. Secondary configuration borrows its token and
rejects missing mappings. Initial held reconstruction restores architectural state only;
final resume rebases the common timer epoch before restoring each PMU, then reapplies
the same epoch. Errors discard the reconstructed VM. The signed `pmurecovery` diagnostic
seeds distinct settings/cycle epochs and enabled 500 ms physical/virtual timers on two
guest CPUs. It destroys the source and restores the same checkpoint twice, each after
a 750 ms held callback, verifying retained controls/cycles, timer delay followed by
expiry and resumed progress. Its 2^30-cycle margin catches held-interval aging on the
verified host; pressure/other-host timing remains unverified. Complete signed group and
private fault/timer/phase checks pass. A second disposable clone of the installed Windows
fixture verifies this integrated path: source boot/capture, source hardware teardown,
matching held pixels, synthetic Space to sign-in and thirty seconds of resumed activity.
Disk requests advance from 15,240 to 18,362 and keyboard requests from zero to four.
Frames were visually inspected; original disk inode/size/mtime remain unchanged. This
does not establish Windows-specific PMU semantics or open-application preservation.
Durable snapshots, worker/agent endpoints and open-application recovery remain pending.

`hypervisor::state::clock` now samples a frozen guest counter at a caller-supplied
common host tick. `restore_frozen` reinstalls its offset before enabling saved timers.
With the native GIC on the verified host this offset shifts both CNTPCT and CNTVCT;
both compare values must remain in the saved guest epoch. Do not shift physical CVAL
again. The signed `clock` probe verifies both enabled 500 ms timers remain unexpired
after 750 ms VM destruction/recreation, then expire after their restored delay. It
also checks guest physical-counter/host-epoch agreement before capture. Pure checks
cover host-epoch replacement and 64-bit wrapping; the existing signed CPU-state
round trip still passes. PMU rebasing, counter-frequency compatibility, multi-CPU
coordination and complete snapshot integration remain pending.

`runtime::run_captured` now binds one CPU capture group to one runtime lifetime.
Requests are admitted only during a coordinated pause, limited to one pending request,
two owner-thread slots and a five-second collection deadline. Both CPU owners capture
their own architectural state with one host clock epoch. Pause generations scope
publication; resume, cancellation and binding teardown discard unfinished captures.
Completed buffers own no VM hardware. Groups cannot bind a replacement runtime.
The signed `group` probe verifies two active CPUs, stable RAM/registers, shared clock
epoch, repeated capture, exclusive admission, dropped requests, resume invalidation,
new pause generations, retirement, single-CPU capture and callback-error cleanup with
VM reacquisition. Existing signed pause/error/panic checks also pass. Pause observers
run without holding the pause mutex and recheck generation/stop before acknowledgement;
seven pure pause tests pass. The checkpoint now also includes RAM banks and GIC state,
but lacks the remaining devices and disks and is not a restorable VM snapshot. No
worker/agent capture endpoint is enabled. Those components, PMU handling, persistent
runtime integration and complete recovery publication remain pending.

Native pause now acknowledges only at quiescent secondary-owner boundaries. A queued
CPU start is applied before the parked-owner checkpoint; an outstanding trap request
must receive and apply its reply before the execution checkpoint. The primary pumps
one bounded secondary trap per pause-wait iteration and verifies the request channel
is empty after acknowledgement. Guest shutdown/reset during pumping ends the runtime
instead of falsely publishing Paused. The signed capture probe now exercises enabled
UART writes from CPU 1 while checking repeated stable CPU/RAM capture and resume.
Seven pure pause tests cover reply-draining acknowledgement, shutdown interruption,
generation cancellation and observer behavior; signed pause and CPU_ON/OFF probes pass.
Quiescing avoids serializing half-applied trap replies. RAM/devices/disks, PMU
preservation and complete restore integration remain pending.

CPU checkpoints now include `snapshot::Software`: PSCI online/parked state, the exact
affinity/executable-region topology, and each CPU's emulated OS debug lock. Pending
starts and a stopped primary reject capture. Software restore validates topology,
executable ranges, debug count and absence of pending starts before mutation; it does
not recreate CPU owners. Capture collection waits for both architectural owner slots
and primary-owned software state under the same request/pause generation. Three pure
checks verify online/parked/debug restoration and rejection without mutation. The signed
group probe captures a real guest-written CPU 1 debug unlock; its parked-CPU case captures
a never-started secondary and restores its software power/debug state into fresh metadata.
Actual restored worker ownership, RAM/GIC/devices/disks, PMU state and complete VM
snapshot/restore publication remain pending; no agent endpoint is enabled.

`hypervisor::ram::capture` now owns a private Mach copy-on-write RAM mapping with
maximum protection reduced to read-only. Its lifetime is independent of the source
allocation and VM; reads are bounded to 64 KiB and reject invalid ranges. Callers must
hold all CPU/device writers at one checkpoint boundary during capture. A signed `ram`
probe verifies two captures of populated 32 MiB RAM, concurrent reads during continued
guest writes, production host/DMA write isolation, VM destruction, bounds and copying
captured bytes into a fresh VM for guest reads. Local capture took roughly 2–4 ms;
maximum-size pause latency and memory-pressure behavior remain unverified. A separate
signed Mach region probe observed shared resident pages and SM_COW on the captured
mapping. Strict machine all-target Clippy and the machine suite pass. The RAM owner is
now wired into coordinated CPU/GIC capture, but remaining device/disk capture, durable
snapshots and agent endpoints are unfinished. Full VM restoration and open-application
verification remain pending.

Coordinated capture now publishes owner CPU slots together with primary-owned
`snapshot::native::State`: software power/debug state, all three firmware/NVRAM/RAM
COW mappings and opaque GIC state. Requests remain unarmed until their requesting
callback and interrupt updates finish. CPU owners ignore unarmed requests. While
collection is unfinished, the runtime defers callbacks and interrupt updates, within
the five-second collection bound; failed/expired collection makes callbacks available
again. Completion waits for every CPU slot and the primary bundle. Cancellation,
resume and retirement discard unfinished bundles without retaining hardware. The signed
group probe verifies final requesting-callback RAM writes enter capture, repeated banks
remain immutable during subsequent guest execution and after source VM destruction,
and the collected GIC state restores with both fresh CPUs created before execution.
Strict machine all-target Clippy, the machine suite, and signed pause/power-cycle/frozen
clock regressions pass. Other devices, disks, durable recovery and
full live application-preserving restore remain pending; no agent endpoint is enabled.

Primary checkpoint device state now includes `flash::snapshot::State` and an owned UART
clone. Flash metadata captures geometry, block protection, read mode/status and every
pending command, without copying bank bytes during capture. Reconstruction creates a
fresh bank using the matching captured NVRAM bytes, rejects size mismatch and preserves
pending buffered words independently of the source. Two pure tests continue every
command boundary across word/buffered programming, erase, locks, query/identity/status,
capture reuse and rejection. UART coverage verifies configuration/FIFO independence
after source mutation/destruction. The signed group probe now pauses an actual guest
mid-buffered flash write, captures, resumes the original write to completion, destroys
the source VM and independently completes the saved pending write with different data.
It also verifies guest-written UART settings. Remaining backend contents, disk state,
complete runtime reconstruction and live application-preserving recovery remain pending.

`virtio::queue::snapshot` captures only quiescent rings, preserving indices and delayed
initialization. Restore validates regions, available capacity and the initialized used
index against matching RAM without writing it. `virtio::pci::snapshot` captures transport
configuration, BAR/probe/capability layout, features, per-queue registers/state, ISR,
MSI-X, configuration-window state and diagnostics. Restore validates backend family,
offered features, immutable PCI layout and every ring before destination mutation.
This does not restore backend contents or authorize disk/media identity. Primary
checkpoints now capture transports by their seven fixed device slots; any unfinished
request rejects the primary bundle. Three queue tests cover pending rejection, no-write
validation, reusable wraparound and initialization. Three transport tests cover progress,
BAR probes, legacy interrupts, rejection before mutation and one masked MSI delivery
after restore/unmask. The signed group probe now retains a real configured keyboard
transport alongside immutable RAM and restores its progress/interrupt after source
VM teardown. Strict machine all-target Clippy and the complete machine suite pass.
Backend state, disk/media recovery, full runtime reconstruction and open-application
snapshot/restore remain pending; no worker/agent snapshot endpoint is enabled.

PCI checkpoint state now also owns bounded input and guest-serial backend copies.
`pci::snapshot::restore` reconstructs these supported backends with the transport;
`restore_transport` remains explicitly partial. Complete restoration validates keyboard/
tablet kind or serial port name, prepares an independent backend and validates every
transport/ring before destination mutation. Disk/optical backends still reject complete
restoration without mutation. Input includes selectors, LEDs, delivered held
keys and pending events; serial includes negotiation flags, name, control packets and
both bounded byte directions. Five transport tests now include queued delivery, release
of delivered keys only, subtype rejection and unavailable-backend preservation. Serial
coverage verifies independent reusable restore of both byte directions and pending
control packets, plus foreign-name rejection. Input coverage verifies LEDs/selectors
after source reset/destruction. The signed group probe reconstructs the captured input
backend after source teardown, delivers its pending event and releases held keys using
a private copy of the queue RAM region; the original COW capture remains unchanged.
It does not restart a complete guest or reinstate host leases/guest authentication.
Storage backends, disk/media recovery, full runtime reconstruction and live
application-preserving restore remain pending; no snapshot endpoint is enabled.

Graphics state now participates in complete PCI device restoration. GPU resources and
backing vectors use Arc ownership; published and linear frames are retained through
Arc frames. Capturing clones bounded metadata/cursor state without copying bulk pixels.
Resource transfer and scanout flush obtain private pixel/frame buffers with fallible
allocation before mutating shared data; attachment/detachment replaces backing ownership.
`gpu::snapshot` preserves resources, allocation accounting, preferred dimensions, scanout,
cached published frames, resource pixels not yet flushed, cursor and linear state. Restore
checks preferred dimensions and all backing/active-layout ranges against matching RAM.
It preserves staged linear registers independently of the currently active layout.
Three GPU snapshot checks cover copy isolation across source mutation/reset/destruction,
reusable restores, cursor, unflushed publication, restored transfers, framebuffer staged
registers and rejection before PCI destination mutation. Existing GPU/linear tests and
strict machine all-target Clippy pass. The signed group probe now captures a configured
framebuffer, changes source pixels/frame while paused, resumes and destroys the source,
then reconstructs the old frame and refreshes it from a private copy of captured RAM.
This does not prove full guest reconstruction or add graphics acceleration. Disk/optical
state, disk/media recovery, full runtime reconstruction and application-preserving VM
restore remain pending; no snapshot endpoint is enabled.

Native Windows block capture now owns private APFS clones made from the already-open
source descriptor, with a private mode-0700 directory and mode-0400 captured image.
Unsupported/cross-volume cloning rejects without full-copy fallback. Bounded reads are
at most 64 KiB. Recovery forks another independent writable image and retains its private
directory through controller ownership. Block state binds capacity, read-only mode, ASCII
identity and writeback mode; writable capture syncs the source before cloning. PCI capture
and the coordinated primary checkpoint include this backend. Recovery never overwrites
its placeholder target. Disk drop explicitly unlocks its owned flock even with duplicate
file descriptors. Five storage/block checks verify sparse retention, inode/path replacement,
source deletion, repeated independent recovery, immutable captures, capacity/identity/mode
rejection, read-only guest writes and explicit lock release. A signed two-CPU group probe
captures a configured 64 MiB sparse disk with queued-I/O progress, changes source sectors,
resumes and destroys the VM, then repeatedly recovers the old sectors through PCI queues
and writes independent recovered contents. Full machine tests and strict all-target Clippy
pass. These images are ephemeral, without durable publication/recovery receipts. Capture
uses synchronous fsync/clone on the owner queue; the collection deadline cannot interrupt
these syscalls and does not prove bounded pauses under I/O pressure or at maximum size.
Optical recovery, complete runtime reconstruction, PMU handling, durable snapshots and
application-preserving restore remain pending; no worker/agent snapshot endpoint is enabled.

Optical backend capture now includes an independent private APFS image plus negotiated
sense/CDB sizes, pending sense, inserted state, command counters, error/queue counters and
last missing/control/transfer diagnostics. `storage::Image::fork_readonly` creates fresh
mode-0400 read-only descriptors with independent file positions; SCSI reconstruction retains
the private directory and never reopens the original source path. Restore validates target
capacity before preparing a replacement. PCI and coordinated captures now support every
current native backend on Apple silicon. Optical tests verify source mutation, path replacement,
reset/deletion/destruction, reusable state/media recovery, read-only writes, pending-sense
consumption, configuration and rejection before PCI mutation. A signed two-CPU group probe
captures optical queue progress and ejected/error state, changes the source controller, destroys
the VM, then repeatedly restores negotiation, pending sense, ejection and captured-sector reads
through the reconstructed PCI queue. Full machine tests and strict all-target Clippy pass.
These independent device reconstructions do not recreate a complete running VM. Durable
publication, runtime owner recreation, PMU handling, maximum-size/pressure pause bounds and
application-preserving restore remain pending; no worker/agent snapshot endpoint is enabled.

`runtime::run_restored` now reconstructs an in-process checkpoint into fresh owned hardware.
Boot supplies matching topology and timeout; captured firmware/variables replace its bytes.
Preparation validates CPU count, RAM banks and device-slot presence, restores all memory in
64 KiB chunks, flash mode/protection, UART, software power/debug and each PCI backend. Firmware
is RX and NVRAM mapping protection follows the restored flash mode. All native CPU owners exist
before GIC restoration; CPU architectural/ICC state is restored on each owning thread before
any guest execution. Primary/secondary affinity is checked after restore. Secondary recovery
has separate acknowledgements for architectural state and the shared resume epoch. The first
callback is paused; Continue rebases both counters on one fresh epoch and releases the gate.
Restored online CPUs continue their saved PC; off CPUs remain parked and retain ordinary CPU_ON
startup/reset behavior. Stop/error before release closes recovery channels and joins ownership
without executing guest code. No persistent variable store or original disk is changed by this
low-level reconstruction path. Signed group checks cover repeated two-CPU execution from saved
loops with UART traps, restored graphics/input state and disk/optical queue I/O, held callbacks,
single-CPU execution, parked CPU_ON, CPU/RAM/device topology and affinity rejection, invalid GIC
rejection and early stop/error cleanup with reacquisition. Full machine tests, strict all-target
Clippy and signed pause/secondary/frozen-clock regression probes pass. PMU handling, durable
publication, host-side pending-action recovery, restored-runtime recapture, worker/agent routing,
maximum-size/pressure timings and actual desktop application preservation remain pending.
Memory reconstruction currently copies rather than remapping COW. No snapshot endpoint is enabled.

Checkpoint state now also includes `snapshot::actions::State`: the bounded 64-byte UART
tail and optional remaining boot-key delay, at most one second. The primary captures these
before synchronous storage work. Recovery retains the tail immediately and installs the
saved relative deadline only on first Continue; automatic key actions never dispatch while
paused. Pure tests verify absent/overdue actions, reusable restoration and bounds. A signed
single-CPU probe emits the real boot marker, captures its pending action and configured input
queue, destroys the VM, holds recovery for 1.2 seconds, then verifies the original remaining
delay and exactly one Enter press/release with SYN events through the restored queue.
`runtime::run_restored_captured` binds a fresh one-use capture group to reconstructed hardware.
The initial staged pause cannot capture until first Continue; a later coordinated pause uses
the ordinary drain/arming/collection path. A signed two-CPU probe resumes, pauses and captures
a reconstructed VM, destroys it, then repeatedly restores that checkpoint with disk/media
queue I/O and both saved CPU loops, while the original checkpoint remains immutable. Full
machine tests and strict all-target Clippy pass. This remains an in-process foundation;
durable publication, PMU handling, worker/agent endpoints, maximum-size/pressure timing and
live desktop application preservation remain pending. No snapshot endpoint is enabled.

`hypervisor::ram::restore` now creates a private writable mapping with `mach_vm_copy` before
HV admission instead of copying each captured byte. It retains the capture's maximum read-only
protection, validates destination geometry/address width and releases partial host mappings on
failure. An occupied guest range rejects without replacing its existing mapping. Runtime
preparation uses this for firmware, NVRAM and RAM; flash's emulation byte bank remains a separate
64 MiB reconstruction. Signed populated-RAM checks verify guest reads/writes, host/DMA writes,
immutable captures, failed overlapping admission, repeated recovery and survival after both
capture mappings are dropped. Sparse 1/8/64 GiB probes verify boundary/hole reads, isolated
writes, recapture and dropped-owner survival, guarded against large peak-RSS growth before
larger cases. A separate native region probe reports SM_COW with shared pages and zero private
pages after HV mapping. The first 64 GiB probe failed because default IPA width was 36 bits:
RAM starts at 1 GiB, so its end exceeds that 64 GiB address range. VM configuration now queries
the supported maximum and explicitly selects min(maximum,40), requiring at least 36. Every
ordinary/restored memory mapping validates alignment, overflow and end against this saved
configured width before allocation. The final sparse 64 GiB guest probe passes on the local
40-bit host, including out-of-range/overflow rejection. Full machine tests, strict all-target
Clippy and signed group/pause/secondary/frozen-clock probes pass. Sparse/max-RSS evidence is
not populated maximum-size or memory-pressure proof. Lower-width hosts, installed Windows
boot after this configuration change, PMU handling, durable snapshot publication, endpoints
and actual desktop application recovery remain unverified or unfinished.

The signed `desktoprestore` example now verifies real installed Windows boot and in-process
running recovery after the address-width/COW changes. It accepts only a private disposable
`hopperrecovery119.*` temporary fixture, opens its variable-store copy, locks its disk copy,
boots with the normal native device layout, pauses/captures after sixty seconds and destroys
source hardware before reconstruction. Held restored pixels must match the captured frame
hash before execution. The recovered guest then runs for thirty seconds and receives one
synthetic Space press/release, with frame exports and device-fault checks. Local live evidence
shows the Windows lock screen before capture, identical source/held frames, sign-in UI after
recovered input and disk requests advancing from 15,233 to 18,271. Reconstruction reached the
first held callback in about 44 ms locally; this is not a worst-case bound. Source preparation
held original operation/runtime/disk/variable locks while cloning their open descriptors into
private APFS files. Original disk inode/length/mtime remain unchanged after the run. Strict
machine all-target Clippy passes. This closes the local installed-Windows boot regression for
the new IPA configuration and establishes one real OS lock-screen capture/reconstruction
round trip. Open applications, PMU semantics, durable publication, worker/agent endpoints,
full app/viewer integration and repeat/recovery-failure coverage remain release gates.

User VMs are managed by `engine::machines`, separate from the managed Docker VM.
The app routes Windows ARM64 and new Ubuntu VMs through owned native runtimes.
QEMU and swtpm are not bundled or launched. Untagged Windows records require explicit
migration even without a legacy instance directory; preserve their records and disks.
The previous helper must not operate on either native or legacy Windows records.
Native Linux and macOS admission attach virtio NAT using a stable local unicast address
derived from saved VM identity. Mac core configuration also supports a disconnected attachment.
The VM library persists per-VM NAT/disconnected settings in private bounded identity-bound
documents. Changes require operation/runtime exclusion; stopped owned hardware must be
detached and retired first. Linux and Mac preparation freeze the mode under runtime ownership.
Malformed or foreign settings reject instead of reconnecting. Signed SDK probes validate both
persisted modes and blocked replacement while owned. Mac traffic, live switching and localhost
forwarding remain unverified or unfinished.
Native VZ core configurations accept up to 16 named shared folders with explicit read-only
or read-write access. Directory handles retain the authorized inode; canonical-path identity
is rechecked at configuration and startup/installation. Handles remain alive through viewer
and SDK completion ownership. The signed Linux sharing diagnostic verifies guest reads,
writes, read-only rejection, symlink containment and host-path replacement isolation.
Replacing a running share's host path may make it unavailable; it must not redirect access.
The library picker saves bounded private grants bound to VM id, canonical path, device and
inode. New folders default read-only and have editable guest names; mode changes and removal
require operation/runtime exclusion and retirement of stopped hardware. Preparation opens
and freezes all grants before storage allocation; replacement directories reject admission.
Missing grants can be removed without opening their old paths. No MCP host-folder grant tool
is exposed. UI selection, naming, mode persistence across reopen and removal are verified
against an isolated synthetic profile. Signed Linux/Mac admission probes verify a real shared
filesystem device and mutation exclusion. New Ubuntu seeds install a root-owned conditional
virtiofs mount service at /mnt/hopper with nosuid,nodev and a ~/Shared link to a root-owned
guest-local index at /run/hopper-shares. Named links avoid metadata queries against Apple's
synthetic virtiofs root, which Files treated as broken in installed Ubuntu. Unknown index
contents reject without deletion; entries remain bounded to 16. The signed diagnostic exercises
the production helper, no-device skip, repeated mounting, ordinary UID access and index validation.
Installed Ubuntu service activation, ordinary-user reads and read-only rejection are verified.
A local-index prototype opens in Files; updated provisioning in a fresh installed desktop,
existing-guest updates, Mac guest access and Windows sharing remain pending.
Native Linux and macOS also persist a private bounded identity-bound speaker setting,
defaulting on. Changes require operation/runtime exclusion and retired stopped hardware;
preparation freezes the choice before storage allocation. Enabled hardware has one host-output
virtio sound stream; disabled hardware has no audio device. No microphone input is configured.
Signed Linux diagnostics verify playback-device discovery without capture and disabled-device
absence; signed admission checks cover persisted device counts and mutation exclusion. Audible
playback, installed desktop audio, microphone opt-in and Windows audio remain pending.
New macOS creation persists native VZ records. Library Start discovers and downloads supported
official restore media when no local image is selected, then inspects it, prepares hardware, opens its dedicated viewer and installs through
the owned queue. Creation offers a local restore picker and automatic acquisition; profile
changes clear selected media and enforce the macOS 64 GiB disk minimum.
Local restore inspection stages private APFS media,
retains original policy/stop intent and keeps staging/operation ownership through SDK callbacks.
Inspected restore results prepare private native platform state and admit stopped hardware on
the main thread. Identity, requirements and auxiliary firmware remain bound to the selected
restore build. Normal Start is blocked until successful native installation. Inspected preparation can start
owned queue installation with bounded progress, caller cancellation and original-policy
checks through SDK acknowledgement. Installed readiness is persisted only through authorized successful SDK completion and is
bound to the saved platform. Interrupted or written untracked disks require recovery. System
preparation validates the receipt and admits installed hardware without installer media.
Library setup reports bounded phase/progress updates and exposes cancellation without waiting
for the retained operation lock. Original policy and stop intent remain frozen through installed
startup. Direct installation wakes the sleeping queue owner. Valid IPSW installation, system
boot and actual viewer rendering/input remain unverified. Previous untagged Linux/macOS records keep the
Lima/VZ recovery path. Previous QEMU/swtpm Windows instances remain
preserved for migration and are not started through the native path. All desktop guest families are experimental. Release hosts remain
Apple silicon macOS 26+.
Each VM has a dedicated viewer. Agent access defaults on for new VMs and clones
inherit it. Native Windows key chords and pointer actions use connection-scoped input leases shared
with the viewer. Guest tools and composed text input remain pending. Leases are tied
to both the worker generation and a persisted agent-policy generation. MCP lists and creates
Windows through the native facade. Screenshots connect to the app-owned native registry;
other Windows tools return an explicit unavailable error after authorization, preventing
fallback to the previous runtime. The app hosts a same-user private Unix socket under
`machines/agents/native.sock`; HPA1 accepts status, capture, leased control, pause, resume and stop. Headers are limited to
4 KiB, frames to 64 MiB, connections to 16 and buffered captures to one. Policy is rechecked
before dispatch, after completion and during pixel delivery. Peer credentials and private
socket/directory ownership are checked on both sides. The service does not retain the VM
registry while idle, cannot accept boot paths, and is removed on host teardown.
Disconnected requests cancel while the worker protocol drains. Inside-guest connections,
remote startup and guest-tool transport remain pending. Pause/resume/stop operate only
on the registry-owned worker; stop waits for process exit and ownership cleanup.
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
and operations. Mutating requests carry the caller's policy generation and check it
before ownership acquisition or lifecycle dispatch. Original request policy also scopes
queued captures. Legacy status/capture requests may omit it; mutations must include it.
Exhaustion still permits revocation and prevents re-enabling access.
Prototype MCP operations recheck the persisted access setting, use per-VM
cross-process locks, and expose only guest files/input. Snapshots require a
stopped VM and use APFS copies, with a rollback snapshot on restore.

The major release replaces desktop VM lifecycle and viewers with owned runtimes:
direct Virtualization.framework for Linux/macOS and Hypervisor.framework for Windows.
Do not add QEMU to the replacement. `PRODUCT.md` contains the full release gates.
The direct VZ foundation is in `machine::vz`: main-thread-bound ownership, generic
identity, persistent EFI variables, raw disks, read-only installer media, desktop
graphics/input and bounded asynchronous lifecycle completions. It supports EFI and
direct uncompressed ARM64 kernel boot; prior image formats require migration. Optional
serial output uses a duplicated descriptor owned by the framework; callers must bound
its sink. Signed diagnostics verify real framework start/pause/resume/stop and a verified
Alpine initramfs shell. Host lifecycle authorization now connects to an app-owned main-queue
registry. Linux preparation and main-thread admission now persist a private sparse disk,
generic identity and EFI state together. Host exposes preparation. Already-admitted VMs
now route View and start/stop through a dedicated native window and the authorized owner.
New Ubuntu records explicitly select Virtualization.framework and the app routes first
start through automatic media acquisition, preparation, main-thread admission and its
dedicated viewer. First start now stages an EFI installer with unattended boot flags and
private provisioning media. An isolated signed app profile verified complete official Ubuntu
acquisition, unattended installation, automatic installed-system transition, desktop rendering
and normal-user UID 1001 without sudo. Administrator login and guest tools remain unverified.
Previous untagged records keep their recovery
path; new macOS creation and library setup use the native path. macOS has SDK restore-image discovery
and platform configuration with image-specific CPU/RAM minimums, independent
Mac identity, boot loader and graphics. Its auxiliary directory publishes firmware state
and hardware-model binding together with exclusive rename; opening it checks private
ownership and exact model consistency before framework configuration. Existing state is
never replaced. Local restore inspection validates regular bounded files through the SDK
and checks the returned URL against the requested file; invalid media is preserved.
The installer controller borrows the VM exclusively and stays on its queue. Dropping its
handle requests cancellation; callback ownership retains the installer and hardware until
completion, and the VM rejects lifecycle changes while installation remains active.
Automatic acquisition uses SDK discovery, official HTTPS without redirects, exact size and
strong ETag-bound conditional resume. Private cache receipts store SHA-256 for reuse integrity;
this is not an Apple-published checksum. Staged SDK metadata must match discovery before
admission. Original authorization is polled through stalled network requests, partial inode
locks survive outstanding async writes, and written recovery disks reject before discovery.
Signed live checks verify official metadata and a 1 KiB conditional range; isolated app QA
verifies discovery and storage preflight without transferring image bytes. Full IPSW download, successful installation,
valid-media cancellation, first boot and live viewer verification remain pending.
The VZ queue bridge accepts at most 16 queued lifecycle requests and processes at most
16 per tick. The main-queue owner retains each VM across viewer lifetime, rejects
concurrent transitions and retires only stopped hardware with no pending operation.
Cancelled queued requests do not dispatch; started transitions retain native hardware
through completion. Host requests retain the original agent policy generation and a
per-VM operation lease through queued dispatch and native completion. Policy checks run
before dispatch and completion publication; off/on changes invalidate old requests.
The app owner sleeps on queue notification while idle and pumps completions on its main
queue while active. The main app now requires the virtualization entitlement. MCP routing and
complete dedicated viewer integration remain pending; the bridge is not an agent endpoint.
The app caches one native viewer per admitted VM; closing its window hides it without
retiring hardware. The display initializes AppKit, remains on the main queue, retains
native hardware/runtime ownership and detaches on drop. Duplicate display attachment and
retirement with an attached display are rejected. System hotkeys remain with the host.
Automatic guest-resolution changes stay disabled: an unattached-view teardown probe
crashed in the asynchronous SDK reconfiguration callback. Geometry, resize completion,
real window close/reopen, rendering/input and full signed app verification remain open.
Native Linux admission configures a virtio network device with Apple's NAT attachment.
Its local unicast MAC derives from the persisted generic identity with the versioned
`hopper.vz.network.v1` SHA-256 domain; preserve that derivation across retries. The core
also supports a disconnected device configuration, but app settings and live switching
remain pending. A signed Alpine diagnostic verifies guest DHCP, DNS, a public metadata
download and a disconnected link. This does not prove Ubuntu installation or guest tools.
Ubuntu provisioning now builds bounded ISO9660/Joliet NoCloud media entirely in Rust.
Plans contain separate normal/admin account hashes, an explicit guest disk target and
normal-user desktop login configuration. Linux credentials use `machines.linux` keychain
keys; existing `machines.windows` keys stay unchanged. Prepared VMs can attach private
read-only seed media. Cancellation cleans unadmitted seeds; admitted hardware, callbacks
and surviving displays retain seed lifetime with runtime ownership. First start now selects
unattended preparation: validate Ubuntu Desktop sources and ARM64 kernel, clone the ISO
into a private stage, and replace only its existing GRUB configuration with bounded padding.
APFS clones avoid full media copies; unsupported filesystems require preflighted copying.
Retain EFI boot for ARM64 installer/bootloader compatibility. The official cache stays
unchanged. Written target disks reject unattended preparation, and an admitted installer
allows only one hardware start. Completion detection, installation recovery/system-boot
transition, actual account creation, guest tools and desktop readiness remain unverified.
`Machine.runtime` is optional for previous JSON records; new Ubuntu/Windows records
explicitly select virtualization/hypervisor. Reject incompatible guest/runtime combinations
and native records must never invoke previous helpers. Original runtime choice is captured
and rechecked with policy through queued VZ operations. Native listing holds the operation
lease while probing runtime ownership, skipping the probe during another operation. Free
new records are Not created; prepared, unowned records are Ready to start, without claiming
stopped hardware or installed desktops. Previous guest data always requires migration.
Linux preparation captures the original policy before blocking work, retains operation
ownership through admission/start and binds runtime ownership to the VM and outstanding
SDK callbacks. Private state publishes with exclusive rename and retries preserve identity
and EFI variables. Ubuntu installer preparation downloads the pinned official ARM64
Ubuntu 24.04.5 desktop ISO when no local installer is supplied. Its private content-addressed
cache verifies exact size and SHA-256 before exclusive publication or reuse, resumes
interrupted HTTP ranges and retains the original authorization through hashing/download.
The production client uses HTTPS with redirects disabled. Low storage rejects downloading
before VM state creation. Full official acquisition, independent checksum verification,
installation and automatic installed-desktop handoff are verified in an isolated app profile.
Local ISO fallback checks its volume descriptor, which does not prove
architecture or official origin. System boot rejects unwritten disks but does not establish installation readiness. Records with native state
never fall back to the previous helper. Host listing queries admitted VMs on their queue
for actual state and lifecycle/install busy status, without acquiring operation/runtime
locks. Status checks the original agent generation through dispatch and publication.
Unowned instances remain unavailable; no stopped state is inferred from missing ownership.
These diagnostics do not establish a usable desktop.
Keep prototype records intact.
Native parent/worker startup now carries the persisted canonical non-nil `machineId`.
Configuration derives it from the saved record; the session owner checks it against its
locked VM identity after preparation and before launching. Parent and worker both reject
malformed identities before allocation; a missing boot identity is a schema error.
`Started` must echo the same `machineId`, and mismatched/missing receipts reject startup
and reap the worker. Parent and worker must come from the same build; older private
protocol peers fail closed without rewriting VM records or disks. This is startup identity
binding, not proof that boot paths belong to that identity or completed snapshot configuration
binding. Signed `scripts/check/worker.py` checks the real two-CPU worker identity echo,
status, pause/resume and stop; it does not establish desktop readiness. Complete checkpoint
configuration binding, production package ownership/publication, rollback/clones and
worker/agent snapshot endpoints remain pending. Contract and worker rejection checks,
native/startup/install/reboot/session/input-owner/remote/preparation tests pass, including
foreign receipt reaping, lock release and valid retry. App tests compile; no new app UI
run or installed-desktop snapshot claim follows from this startup identity change.

Native startup also freezes a versioned SHA-256 hardware configuration from its actual
prepared `Boot` and devices before guest execution, and requires the nonzero 32-byte
fingerprint in `Started`. `Client` retains it with that worker generation. The shared
`snapshot::configuration` helper binds canonical VM identity, CPU/RAM, exact firmware
bytes and ordered device fingerprints. PCI fingerprints normalize only guest-writable
command/status/IRQ/MSI-X control and omit BAR runtime addresses/probes, matching the
existing compatibility contract. Device fingerprints include fixed PCI geometry, offered
features, queues/vectors, disk serial/capacity/access, preferred display size, input kind,
optical capacity and serial name. Mutable EFI variables, timeouts, image contents,
writeback, guest input/graphics/serial state and negotiated transport state are excluded;
captured component/image decoders still validate those separately. Bump the configuration
domain when the fixed platform/MMIO contract changes. This fingerprint is compatibility
metadata, not authorization or proof that host paths belong to a VM.

The fresh-process group save/load diagnostic now freezes the shared fingerprint before
capture and derives its expected fingerprint from fresh hardware before package opening,
while keeping explicitly synthetic fixture VM/snapshot IDs. Signed worker checks reopen
identical hardware in independent processes and verify stable fingerprints, then detect
CPU and firmware changes. Unit checks cover hardware differences, mutable-state stability
and every authorization callback; missing/zero/malformed startup hashes reject and failed
startup reaps/releases ownership before a valid retry. Native/session/install/reboot/input/
remote/startup tests, strict all-target lint, app test compilation and signed complete
fresh-process checkpoint recovery pass. Production worker/agent package publication,
checkpoint ownership, crash recovery, rollback, clones and open-application checks remain
pending; forwarding a fingerprint does not implement snapshot endpoints.

The native worker now uses `runtime::run_persistent_captured`, retaining its exclusive
firmware store borrow while binding a coordinated capture group to the actual CPU count.
Normal NOR commits still flow through the original durable journal; checkpoint ownership
does not replace the persistent store. No worker/agent snapshot command is enabled yet.
The two-CPU group source now also runs through this persistent path and checks that its
stopped firmware bytes equal the durable store before dropping both owners. The pending
buffered-flash probe uses a real private store: capture retains its pre-commit bytes and
control state, source resumption commits the full NOR word pair, reopen observes that
commit, and independent restored/manual continuation cannot change the durable source.
`scripts/check/vmstate.py` now runs the full signed group suite (including this probe)
before separate-process package save/recovery. Store setup exposed a fixture timing
assumption: its old 100 ms clock started before storage preparation and could pause before
guest counters advanced. The diagnostic now waits for observed execution on both CPUs before
capture; the runtime deadline still rejects guests that never execute. The failed fixture
`/var/folders/__/c5cv93yj28g33k_s3ccyk3_00000gn/T/hoppervmstate.9zub3a28` is preserved.
This does not implement persistent restored-store publication, crash recovery, rollback,
clones or authorized worker/agent snapshot endpoints.

`snapshot::publication` now owns one background checkpoint writer per handle. It moves
an immutable captured cut and pinned parent descriptor to the thread, retains a supplied
ownership token through completion delivery/teardown, and applies the original caller's
policy/runtime check before admission, throughout serialization and before/after taking
cached completion. Observed revocation, explicit cancellation or deadline expiry permanently
cancel that handle; off/on cannot revive it. Polling returns without joining a running
writer. Drop cancels and joins before releasing its token, including error/panic paths.
Synchronous filesystem calls can delay cancellation and teardown; this is not a hard I/O
deadline. Cancellation after exclusive rename may leave a complete package and never
permits replacement. Callers still retain actual VM operation/runtime ownership and enforce
per-VM exclusion; the generic token is not proof of those grants.

Snapshot `Stage::publish`, complete checkpoint save and background start now require an
explicit commit-guard factory. After writing/syncing the manifest, publication acquires that
guard and rechecks the original authorization before exclusive rename; the guard remains
held through parent sync and the final check, including error paths. A production caller
must use the same `.access` lease as policy mutation and the original policy generation;
periodic checks or a heartbeat cannot fence the rename. Package tests exercise real flock
contention from independent opens and a separate process during commit, revocation between
checks and guard acquisition, busy-guard rejection, duplicate publication and post-rename
failure cleanup. Diagnostic callers explicitly supply a no-op guard because their synthetic
policies have no independent mutator; this is not production worker policy integration.

The host-side `engine::machines::native::snapshot::Policy` now captures the original agent
request generation (or a separate trusted-person policy). Checks read bounded no-follow
saved records, require matching canonical VM identity and the Windows Hypervisor runtime,
and share permanent failure across cloned callbacks. `commit` takes the manager's actual
`.access` lease and rechecks under it; the returned owner releases that lease explicitly.
Tests call the real manager access setter from another process while the guard is held and
after release, exercise off/on without an intermediate check, invalid/foreign/oversized/
symlinked records without rewriting, local-person access and real package publication with
policy toggles before commit. The host guard is not yet transported to the worker; it does
not establish operation/runtime generation ownership or expose a snapshot endpoint.

Snapshot staging can now hand off a sealed package without publishing it. `Stage::seal`
checks original authorization before and after manifest sealing, synchronizes the stage and
parent namespace, and returns a fixed-field serializable receipt binding its random nonce,
directory device/inode and manifest SHA-256. `Sealed::open` only derives an internal `.stage`
name beneath an already-pinned private parent, checks that receipt and the host's expected
VM/snapshot/configuration/layout, and retains the directory. Commit rechecks directory and
manifest identity under its guard before exclusive rename; duplicate commits cannot replace
an existing package. A receipt conveys no policy, operation or runtime authority. Component
schemas, payload checksums and disk-image receipts still require restore-time validation;
this does not claim hostile same-user filesystem mutation is excluded.

`checkpoint::stage` uses the complete serializer but leaves final commit to the host.
`native::snapshot::Policy::publish` checks the expected VM binding, reopens the sealed receipt
and publishes under the original saved policy's access lock. Separate-process package tests
seal in a writer that exits before host reopen/commit, plus replacement/symlink/foreign
receipt/manifest rejection, duplicate commit, cancellation and preserved incomplete stages.
The signed full two-CPU diagnostic serializes a real cut through the staging API, drops its
original capture/handles, reopens the receipt, commits and restores all components. The
background save/fresh-process/revocation pipeline and strict machine/engine lint also pass.
Private worker handoff and host capture ownership are now implemented below; crash recovery,
persistent restore/rollback, snapshot indexing and agent dispatch are still unfinished. Existing background publication remains a separate direct-commit path;
production worker integration must use host-controlled final publication.

`publication::stage` now returns `Publication<O, Sealed>` using the same owned writer,
permanent cancellation, deadlines and checked single-completion delivery as direct publication.
Its thread serializes the complete immutable cut but cannot complete publication through this
API. The generic owner remains retained after stage collection; `Publication::check` retains
original cancellation/deadline/policy checks through the caller's commit. New actual-stage
unit tests cover no publication before separate commit, ownership through receipt reopen/
commit, cancellation after collection and permanent cached-stage revocation. Nine writer
lifecycle/staging tests pass, plus strict machine/engine lint.

The signed two-CPU save fixture now stages its second held cut on that background writer
while retaining the first cut for independent validation. Its diagnostic policy callback
gates work until observed progress on both resumed source CPUs; the VM owner never waits
on that gate. Source disk/media/display changes occur after writer admission; CPU/UART
execution continues after resume. The diagnostic owner collects the sealed stage, round-trips
its receipt, drops the returned handles, reopens it and commits under the original job checks
before stopping hardware. That final small commit performs synchronous filesystem calls on
the diagnostic owner, not a separate host process, and uses a no-op commit guard for its
synthetic policy. It is not production worker IPC or host policy-lock transport. The target's
preexisting presence is checked before commit, preserving the deliberate duplicate-publication
rejection. Fresh processes recover the package and independently recapture it without changing
its source.
Save revocation injections at admission, pre-stage and during staged-image hashing reject
without publishing a complete snapshot; the stage/image remains preserved until owned
fixture cleanup. Six lifecycle tests cover admission failure, permanent revocation, drop
join, completed-result policy checks/single collection, explicit cancel/panic and expiry.
Strict all-target lint and signed full group/fresh-process/rejection checks pass.

An initial receipt diagnostic incorrectly required an absent target even in the deliberate
duplicate-publication case. That test failed before commit; the fixture remains preserved at
`/var/folders/__/c5cv93yj28g33k_s3ccyk3_00000gn/T/hoppervmstate.riwoash4`.
The corrected check compares presence against the original namespace; exclusive rename still
rejects the duplicate. The full signed staging/recovery/rejection runner passes after correction.

An initial negative test incorrectly expected callback 200 to fall during sparse firmware
scanning. It actually interrupts `storage0/image` hashing; its failed fixture remains at
`/var/folders/__/c5cv93yj28g33k_s3ccyk3_00000gn/T/hoppervmstate.1ekfmtq_`.
The sparse encoder emits a four-byte length even for zero chunks, so checked output
already brackets those scans; no sparse-authorization defect or fix was established.
Production worker/agent job dispatch, generation-bound publication ownership, crash
recovery, persistent restore/rollback, clones and open-application verification remain pending.

The native worker now accepts private `SnapshotStart`, `SnapshotPoll` and `SnapshotCancel`
commands with a canonical snapshot UUID and original parent-supplied policy-generation tag.
They accept no host paths. The worker pins the private parent of its boot variable store;
staging only opens an internal `snapshots` child with no-follow directory operations. It
retains one job, pauses all CPUs for coordinated capture, stages the immutable cut on its
owned writer, resumes a previously running VM and preserves a manually paused source.
Polling returns bounded capturing/staging/ready/failed status with a fixed-field receipt.
Concurrent jobs and mismatched IDs/generations reject; cancel/stop joins any active writer
and preserves private stages. A ready/failed job remains retained until explicit cancellation.

A five-second renewal lease and five-minute total deadline limit private staging after its
trusted parent stops polling; expiry/cancellation is permanent. This lease does not read or
replace the saved agent policy and cannot authorize publication. Private staging can continue
until lease expiry after parent authorization is lost, and synchronous filesystem calls can
delay cleanup. The host must retain the original request/policy/runtime/operation owners,
renew only while authorized, cancel on failure, and commit under `snapshot::Policy`; no worker
command publishes a package. The model owns the shared receipt/status schema. Parent wire
validation checks returned snapshot identity and fixed receipt fields; public session requests
explicitly refuse these commands; the dedicated capture controller below is the trusted route.

The signed worker diagnostic verifies real two-CPU staging, receipt inode/manifest binding,
source running/manual-pause preservation, namespace symlink rejection, job exclusion, policy
tag mismatch, permanent renewal expiry and stop cleanup. A separate host process commits a
real worker stage while its source process remains alive, decodes every checkpoint record,
and later rejects a second stage after saved-policy off/on. The profile is isolated and
synthetic; it does not prove installed Windows snapshot recovery or production VM operation/
runtime-lock ownership. Model contracts, private-wire failure/public-forwarding tests, package/
worker tests, session/policy regressions, strict model/machine/engine lint and app test compilation
pass. The capture controller now manages host operation/cancellation ownership as described
below; snapshot indexing/UI/MCP routing, persistent restore/rollback, clones and installed
open-application verification remain pending.

`Sessions::snapshot` now owns native capture through worker cleanup. It freezes saved policy
before waiting for the session, retains an owned session guard and per-VM operation lease,
and uses the worker's startup configuration/topology with a namespace capability pinned
before launch. Namespace/store identity and private ownership are rechecked on admission;
replacement rejects before dispatch. `snapshot_with_generation` additionally binds an agent
connection's upstream original generation rather than refreshing it at method entry. Setup/
recovery receipts reject capture; isolated trusted starts without deployment receipts remain
usable for diagnostics. No snapshot name/index or user-facing endpoint is supplied yet.

A detached controller retains those owners if its caller future is dropped. Original caller
cancellation, saved policy, runtime generation/state and a five-minute deadline bracket
queued dispatch, replies and host publication under the access-change lock. It polls/renews
only while authorized and handles an already-dispatched reply drain before cleanup. Commit
runs on a retained blocking task; sync filesystem calls can still delay it. Cleanup uses only
the original worker/job/tag, even after revocation. Worker cancellation is idempotent when
no job exists and joins active writes before acknowledging. Owners are released before the
controller reports completion. An error after rename can leave a valid complete package;
never replace/delete it blindly. Failed cleanup triggers worker stop/reap before release;
if process cleanup cannot be verified, the operation/session remain quarantined and the caller
receives an error. That exceptional quarantine remains owned until app teardown; live failure
of OS process reaping has not been exercised and needs an explicit recovery UX.

Eight controller tests cover cancellation during reply drain and delayed cleanup, blocked
public stop, frozen policy while waiting, upstream off/on rejection, precommit revocation,
post-publication cancellation, rejected start/idempotent cleanup, cleanup failure stop/reap,
source shutdown with stage preservation and startup namespace replacement. Session/policy/
wire regressions total 32 passing primary tests. A signed real two-CPU controller profile
exercises runtime/operation ownership, complete record decoding, duplicate preservation,
revoked-agent rejection and trusted-person capture of a manually paused source. Signed
private-worker checks, strict model/machine/engine lint and app test compilation pass.
Installed Windows applications, all-OS integration, durable indexing, UI/MCP routing,
restore/rollback and clones remain release gates; this capture path does not satisfy them.

Native package discovery now reads the atomic HSM2 manifests through a pinned private
namespace, without a separate index required for finding completed packages. Enumeration has
independent directory cursors, checks original authorization throughout, bounds all entries
(including ignored stages/unknown names) to 4096, and opens only fixed lowercase-hex package
names with no-follow private directory/manifest checks. Foreign or damaged published packages
fail listing rather than silently disappearing. Unknown/staged entries remain preserved.
`Manifest::inspect` validates checksum/schema/inventory for discovery only; its configuration
and topology are saved claims, not independently authorized restore hardware. Restore still
requires expected binding/layout and complete component decoding. Five discovery tests cover repeated cursor use, staged/unknown preservation, foreign/renamed/
corrupt/symlink rejection, private modes/hard links, entry bounds and authorization cancellation.
The manifest/handoff/discovery regressions total 18 passing primary tests. Strict machine/engine
lint passes. The signed host controller independently verifies discovered binding/layout before
full checkpoint decoding and discovers both completed packages after worker teardown. Human
names, creation metadata, production Host/MCP/UI list routing and recovery/garbage collection
remain pending.

Saved-policy `Policy::inventory` now derives the native namespace from the saved canonical
VM identity and pins each native/VM/snapshots directory through relative no-follow opens.
It rechecks directory identity and original policy before returning; absent unprepared state
returns an empty list without creating directories. Host exposes local actor and original-agent-
generation variants. MCP `vm.snapshots` routes native Windows directly to this durable catalog
without a running app/worker or any previous runtime helper. Entries expose canonical snapshot/
VM ids, saved configuration fingerprint, CPU count and RAM bytes. Those are saved claims, not
restore admission. MCP restore remains unavailable; UI list integration remains pending. Four catalog tests, saved-policy regressions and an MCP endpoint
test verify unprepared/reopened discovery (including a separate reader process), no state
creation, namespace symlinks, policy off/on,
trusted-person access, legacy-runtime rejection and preservation. The signed capture profile
also discovers both committed packages through a fresh policy after worker teardown. Strict
engine/host/MCP lint passes. All restore/rollback, names, clones and broader release gates remain.

Native named capture now freezes a trimmed UTF-8 name (1–100 bytes, no control characters)
and host creation time before waiting for runtime ownership. `snapshot_named` and
`snapshot_named_with_generation` share the existing retained capture/cleanup controller.
The host writes a bounded checksummed HSN1 description into the sealed stage before its
exclusive package rename, bound to the VM/snapshot identities and exact manifest SHA-256.
Metadata is mode-0400, synced with its directory, and revalidated against the host-authored
checksum before commit. A failed description/authorization does not publish; unknown/private
stages remain preserved. Older unnamed packages list with null name/time. Present corrupt,
foreign, symlinked or noncanonical metadata rejects listing. Host/MCP catalogs expose optional
`name` and `createdUnixSeconds`; no second index transaction is needed. Cancellation during
post-publication cleanup preserves the completed name and manifest. Snapshot name editing,
UI creation/listing, persistent restore/rollback and recovery remain pending. The signed two-CPU capture check writes a real named package, fully decodes it and
finds its name/time after worker teardown; it is still synthetic idle hardware, not installed
application recovery. Five metadata tests cover Unicode/bounds, checksum/schema/identity
damage, symlinks, missing/changed metadata before commit and cancellation. Package handoff/
inventory and controller/catalog/policy regressions pass; MCP verifies named response fields.
Strict machine/engine/host/MCP lint passes.

Named native agent capture now crosses the app-owned HPA1 socket. Its bounded request carries
only VM/snapshot identities, name and mandatory original policy generation, never boot/host
paths. The service dispatches to the retained named capture controller and bounds the whole
connection to 310 seconds (client 315). Disconnect or pipelined bytes drop the caller future;
the controller still owns session/operation leases through worker reply drain and cleanup.
Original policy is checked before dispatch/publication and response admission. A completed
capture returns its requested identity only after cleanup; an uncertain response does not
remove a published package. Host exposes the scoped client and MCP `vm.snapshot` creates a
canonical identity, sends the named request and returns id/machineId/name. Native restore
remains unavailable, and the catalogue calls capture experimental rather than implying usable
application recovery. Five socket tests cover named capture, missing/stale policy, invalid
names/ids, off/on before ready, disconnect/pipelining and post-publication disconnect. MCP
exercises create-through-external-owner then catalog listing with lock retention through
cleanup. The signed real two-CPU check now captures through the socket, fully decodes the
checkpoint and discovers name/time after worker teardown. UI snapshot integration, restore/
rollback, installed application recovery and all broader release gates remain pending.

Persistent native restore now has a core EFI-store preparation path. Trusted callers can use
`variables::from_snapshot` only for a fresh absolute namespace, with independently admitted
captured NVRAM and flash command state. It validates the fixed 64 MiB/guest-address geometry,
reads bounded 64 KiB chunks under original authorization, validates flash geometry and creates
an exclusive private store with the exact captured bank. It never replaces existing stores.
A failure before creation leaves no store; after creation it preserves the new namespace for
recovery. Pending NOR command bytes are not applied early. The restored runtime entry points
`run_restored_persistent` and `run_restored_persistent_captured` reject poisoned or byte-mismatched
stores before hardware allocation and retain an exclusive mutable store borrow through native
completion. Confirmed restored guest NOR changes use the normal durable journal before guest
acknowledgement. This is a trusted core foundation, not host disk/firmware publication, rollback
or an agent restore endpoint. Three unsigned restore-store tests and existing variable/flash
regressions pass. The signed flash diagnostic rejects the advanced source store, prepares an
independent captured bank, resumes the pending write, verifies durable reopen and unchanged
source/capture. Strict machine/engine lint passes. Atomic host restore transaction, durable
runtime identity/selection, crash recovery, UI/MCP restore and installed application recovery
remain pending.

Persistent disk recovery now has a trusted staged-image capability. An admitted immutable
`storage::Image` can prepare a sparse writable APFS clone under the fixed exclusive `disk` name
in a pinned private caller-owned stage. It never replaces a name or falls back to a full copy.
Original authorization brackets cloning/admission, and inode/capacity/owner/mode/single-link
checks surround writable reopen and final namespace verification. Failure preserves written
stage data. The returned `storage::Prepared` owns an exclusive file lock and explicitly unlocks
on drop even if a trusted caller retained a duplicated descriptor. Writable captured block
states and PCI checkpoint disk states expose this preparation; non-disk/read-only backends
reject. Ordinary runtime reconstruction still forks ephemeral disks. The explicit persistent runtime
route below adopts prepared disks; atomic disk+EFI publication is still unfinished. Five tests
verify sparse independent persistence, cancellation, duplicate lock lifetime, unsafe/existing
namespaces and inode replacement. Storage record regressions and strict machine/engine lint
pass. The signed recovery diagnostic now prepares a persistent disk directly from the captured
PCI backend, verifies captured sectors, writes/syncs/reopens its stage, then independently
recovers the original captured sectors. Host transaction, runtime adoption, rollback/crash
recovery, UI/MCP restore, immutable clones and all-OS installed application QA remain pending.

Trusted persistent recovery now adopts prepared disks into runtime ownership. `Disks` binds
exactly every captured disk slot with an original authorization callback; missing/extra slots
reject instead of falling back to ephemeral disks. Prepared disks retain their source capture
identity and private namespace, compare all logical bytes in 64 KiB chunks under authorization
before hardware allocation and again at backend adoption, and reject foreign/modified/replaced
files. This intentionally scans large disks twice and is not a hard filesystem-time bound.
Trusted callers must retain exclusive stage-writer ownership; advisory locking and comparison
do not exclude hostile same-user writes. Adoption checks controller identity and rejects sharing
the replaced controller's disk inode. It transfers the locked descriptor without an intermediate
unlock; the runtime and returned stopped devices retain ownership until their final drop.
`run_recovered` and `run_recovered_captured` combine this with the matching durable EFI store,
held checkpoint reconstruction and authorization checks around owner callbacks/completion.
Revocation after a callback returns Continue rejects before the guest is released. Three admission tests and storage/block/EFI regressions pass. The signed two-CPU check
verifies persistent disk writes through restored virtio queues, lock retention through returned
stopped devices and reopen after teardown. It rejects missing/changed/foreign/replaced disks
before callbacks and policy revocation after Continue is requested, then verifies lock cleanup.
A full signed pipeline passes source and two fresh reader processes, independent recapture,
authorization/damage rejection and unchanged saved packages. The captured variant now has the signed persistent recapture coverage described below. Strict
machine/engine lint passes. None of this is a host transaction or agent restore endpoint. Host disk+EFI publication/selection, rollback,
crash recovery, UI/MCP restore and installed open-application recovery remain pending.

The signed diagnostic now exercises `run_recovered_captured` with a prepared persistent disk
and EFI store. It verifies held initial CPU state, refuses capture before first Continue,
performs restored virtio writes, waits for both saved CPU loops to advance, pauses and collects
a fresh full checkpoint. The returned devices retain the persistent disk lock until drop;
retired stage writes reopen with the expected sectors. The new checkpoint is sealed, committed,
destroyed and reopened through full component validation. Two independent persistent recoveries
then verify held CPU counters and captured disk/controller queue state, write different sectors,
resume both CPUs and retain their own disk locks until device teardown. The original RAM cut
and published package stay unchanged. Source and two fresh reader processes complete the full
signed pipeline, including existing policy/namespace rejection, independent recapture and
corruption/no-replacement checks. The runner now requires the persistent recapture proof in
each fresh reader output; both completed traces satisfy that added evidence gate. Strict
machine/engine lint and Python compilation pass. These are synthetic guest diagnostics, not
installed Windows/open-application recovery; host transaction/selection, rollback/crash recovery,
all-OS live restore and UI/MCP restore remain unfinished.

Apple-silicon EFI stores pin their private directory instead of reopening their saved path.
Bank reads, compaction, journal/lock opens and fixed-child `create_at`/`open_at` operations
use directory-relative descriptors. `from_snapshot_at` restores captured NVRAM into a pinned
private stage without replacing an existing child. Failure after creation preserves the stage.
Tests cover parent/child rename, replacement paths, compaction, symlink/hard-link rejection,
exclusive creation and permissions. The persistent signed recovery diagnostic now prepares
disk/EFI under a stage and renames it before reconstructed hardware runs. The generation layer
below adds bound receipts and core publication. Host selection, rollback and crash recovery
remain unfinished.

`machine::snapshot::generation` stages prepared disks and EFI under one pinned private
namespace. HGR1 contains a nonnil generation id, the complete admitted HSM2 manifest and a
SHA-256 footer; reopening compares the entire receipt against the independently admitted
source package. All component decoding precedes stage allocation. Publication requires a
caller-acquired original-policy commit guard, validates disk contents and stage/child identities,
and uses exclusive rename. Cancellation after rename preserves a complete generation.
`Generation::open` reloads the original checkpoint against fresh authorized hardware and
re-admits unchanged prepared disk contents with exclusive locks, plus matching captured NVRAM.
The EFI inode comes from its actual retained store, rather than a second pathname open.
`consume` revalidates and retains original authorization in persistent runtime resources.
Publication, reopening and handoff add full logical-disk comparisons, with no hard filesystem
deadline. These APIs are trusted core only. The consumption guard below rejects replay once
resources have been handed out. Unchanged disks do not make an old CPU checkpoint safe to replay.
Host selection, rollback, crash recovery, worker restore protocol, UI/MCP restore and installed
open-application verification remain unfinished.

The signed generation pipeline verifies source capture and two fresh readers, three published
and reopened generation handoffs per reader, persistent restored execution and recapture.
Failures cover revoked preparation without allocation, altered receipts, stage and EFI-child
replacement, denied commit guards, post-rename cancellation preserving a reopenable generation
and duplicate publication without replacement. Sixteen targeted storage/EFI tests, strict
machine all-target lint and Python compilation pass. The runner now exposes rendered compiler
diagnostics on build failure and requires both generation success/failure proof in each reader.
These synthetic checks do not close host transaction, replay prevention or installed-app gates.

Generation resources are now available only through `consume`, replacing the unrestricted
`into_parts` handoff. It validates before acquiring the caller's original-policy commit guard,
then exclusively creates `attempt.hga`, seals/syncs it and syncs the pinned generation directory
before returning checkpoint/disk/EFI resources. HGA1 is 84 bytes: magic, HGR1 SHA-256, fresh
16-byte nonce and SHA-256 footer. Original authorization, component identities and exact marker
bytes are rechecked under the guard before handoff. Any creation/validation failure preserves
attempt state and releases owned resources; guard denial before creation leaves no marker.
All generation admission and publication namespace checks reject any attempt entry using
no-follow fstatat, including malformed/partial files, dangling symlinks, directories and FIFOs.
Marker contents never authorize replay. This is a conservative core guard, not host crash
recovery: the host still needs prior-state preservation, selection, rollback and a durable
worker restore/resume transaction. No automatic marker removal is allowed.

Signed attempt verification passes in the source and both fresh-reader processes: marker
receipt/checksum, cancellation after durable creation, released disk/EFI ownership, no marker
on denied guard, and rejection/preservation of partial files, symlinks, directories and FIFOs.
Each reader requires three replay rejections after complete runtime/device/EFI teardown,
alongside generation publication/reopen, persistent execution and recapture. The full existing
authorization/damage/no-replacement/source-immutability pipeline, sixteen targeted storage/EFI
tests, strict machine all-target lint and Python compilation pass. This does not verify an
actual process-crash transaction, host active selection, rollback or installed-app restore.

`engine::machines::native::restore` adds initial host intent, not a completed restore controller.
HRR1 is a checksummed, exact 149-byte record binding VM, generation, source snapshot,
configuration, rollback snapshot, predecessor and Prepared/Attempted/Active phase. Initial selection
supports the original predecessor only. It checks both package manifests, exact HGR1/source
manifest identity and absence of any attempt entry. Full component/previous-state admission
remains the trusted controller's responsibility; metadata fixtures do not prove usable recovery.
`Owner` captures original policy before locks and retains manager operation/runtime exclusion
plus the journal lease. Policy revocation invalidates its checks without releasing ownership
until teardown. Journal commits use the actual saved-policy access guard. Selection is exclusive;
attempt publication compares the original record and uses synced atomic replacement. Failed
or cancelled publication preserves partial/committed state. There is no automatic record removal.
Native initialization and system/installer preparation reject any restore intent or malformed,
foreign, nonprivate or symlinked record before probing the original target disk. Cold boot from
the selected generation is unfinished, so every phase still fails closed without fallback.

Forty-seven primary restore/startup/session/snapshot-controller/policy tests and child-process
probes pass, including writer exit without destructors, journal/runtime exclusion, postcommit
cancellation, rechecksummed foreign manifests, attempted generations and policy off/on.
Startup gate tests first establish that the original disk can prepare a system boot. Strict
machine/engine all-target lint passes. The signed host snapshot diagnostic fully admits real
source/prior captures, publishes a real core EFI generation, records selection/attempt under
host ownership, consumes it, reopens intent after owner teardown and rejects startup/replay.
Its synthetic guest has no system disk. Active-generation cold startup, rollback, installed-guest
crash recovery, applications and UI/MCP restore remain unverified or unfinished. Held worker and
registry admission are described below.

The parent-only protocol now has StartRestored { boot, restore } and RestoreResume { restore }.
Restore carries canonical snapshot/generation ids and a required original policy generation;
Restored echoes them, machine identity and a nonzero hardware configuration. The parent validates
all fields and rejects ordinary Started as restored admission. Both commands are denied by public
Sessions::request. The worker separates hardware construction from original EFI acquisition,
pins the original VM root, opens snapshots/generations with openat/no-follow, fully admits the
source/generation, consumes the attempt marker and uses run_recovered_captured. It starts held,
announces Restored from Mode::Paused and requires exact RestoreResume before Continue. Ordinary
resume/input/release/setup-finish/snapshot-start reject during admission. Wrong or replayed resume
rejects; a pending hold expires after 30 seconds. Deadline expiry itself is not live-verified.
The snapshot service retains the original VM namespace while runtime EFI remains generation-owned.

Trusted native::launch_restored/resume_restored take an Arc<Owner>, require matching Attempted
HRR1 selection/configuration/tag and retain ownership in background tasks across caller cancellation.
Startup polls the original policy and cancellation while reconstructing without a long access
commit guard. HRR1 is already Attempted; the worker attempt marker is conservative bookkeeping,
not execution authorization. Actual explicit resume retains the original policy commit guard
through its reply. Failed validation/cancellation drains and stops/reaps before returning failure.
The actor retains Owner through process exit even after all clients drop; an unconfirmed reap
quarantines it. No public restore route exists. Prepared registry admission is described below.

Five process-level tests verify startup cancellation, revocation during reconstruction,
configuration rejection, cancelled resume and ownership past stop acknowledgement until actual
process exit. The wider native/model/restore/wire/startup/session/controller suite and strict
model/machine/engine all-target lint pass. The signed real-worker probe verifies held restored
startup, ordinary/stale/replayed resume rejection, matching explicit resume, running status,
stop/reap, attempted-generation rejection, unchanged original EFI-bank bytes and exclusive
ownership after caller-owner drop with release only after process reaping. The synthetic guest has
no disk and does not prove installed Windows or open applications. Active-generation startup/reboots, crash recovery, rollback and UI/MCP restore remain unfinished. Full-disk validation
latency and unconfirmed-reap recovery remain unverified.

Trusted Sessions::start_restored consumes Owner by value, validates registry/VM/pinned-root inode
binding before launch and publication, admits held hardware, explicitly resumes under original
policy and publishes through the ordinary slot epoch/state mechanism. Publication is fenced by
the actual access commit guard. It releases only the preparation operation lease; runtime/journal
ownership remain with the actor through actual reaping. Ordinary guest/viewer requests then acquire
fresh operation leases. Session::finished waits for confirmed reaping and Owner release. Caller
cancellation drops the nested helper future, whose owned task drains/stops the unpublished worker;
registry teardown drops clients while viewer watches remain non-owning. Six process-level tests
cover operations/revocation/shutdown, teardown retention, startup/resume cancellation and foreign
registry/replaced-root rejection. The signed two-profile diagnostic verifies direct held admission
and registry status/pause/resume, viewer-watch publication, stop/reap, runtime release and preserved
original EFI/pending HRR1 state. Sixty-four primary native/model tests and strict lint pass. Its
synthetic guest has no disk; actual viewer rendering and installed applications are not proved.
Automatic source/prior capture and generation preparation,
durable active-generation cold boot/reboots, rollback, app restore UI/viewer and MCP restore remain
unfinished. The prepared API accepts trusted host boot paths; never forward it from an agent packet.

The shared machine::snapshot::selection codec now includes Active (HRR1 phase 3). Held worker
admission requires an exact bound Attempted selection. Owner::activate writes/seals a private
replacement, retains the actual original-policy commit guard across compare/rename/directory sync
and returns that guard to resume_worker for retention through RestoreResume's reply. The worker
independently requires the exact matching Active record before releasing held CPUs. All fields,
including rollback/predecessor, are compared. Failed resume/cancellation preserves Active for
recovery and stops/reaps; config::check_start still rejects every HRR1 phase. Active cold boot is
not implemented and must never fall through to original storage or replay the consumed checkpoint.
Startup validates boot.store against the Owner namespace and polls the original VM inode during
reconstruction. Activation rechecks namespace identity before/after commit. Tests observe Active
before fake resume acknowledgement, retained policy exclusion, cancellation persistence and
namespace replacement rejection. Reopening Active refuses checkpoint replay before creating
another worker. A synthetic-worker process-exit test skips all destructors,
then verifies persisted Active, retained artifacts, EOF worker exit and original-boot exclusion.
This proves host durability/ownership, not installed-guest crash recovery or disk consistency.
Sixty-nine primary native/model/restore tests, child-process probes and strict lint pass. Signed
direct and registry probes verify activation-gated execution and retained Active after teardown.

Core generation::System admits mutable generation disks/firmware for fresh boot, not checkpoint
replay. It requires matching Active HRR1, bounded/checksummed HGR1 manifest provenance and exact
HGA1 binding. No captured CPU/RAM/device payload is decoded or needed. Caller retains original
policy/runtime exclusion and must compare the independently rebuilt hardware fingerprint before
execution. Trusted worker/registry cold-start routing is described below; configured app Start is now wired
to it. Signed synthetic configured-start/reset checks pass below; installed recovery remains pending.
Stage publication now seals/syncs a fixed 387-byte HGO1 ownership receipt before rename. It binds
HGR1 SHA-256, generation and firmware-directory device/inodes, plus all seven disk-slot presence,
directory/disk device/inodes and capacity. Generation and System check it against pinned handles;
System also rechecks HRR1, HGR1, HGA1 and all parent/child namespaces before handoff. Missing HGO1
or same-size replaced storage rejects without repair. Previous experimental generations need
explicit recovery; copies/clones must regenerate identities under their own authorized transaction.
storage::Current uses private no-follow bounded regular single-link mode-0600 disks, retains their
exclusive lock and pinned directory, and transfers to a fresh block controller without restoring
old queue/writeback state. Drop/failure explicitly unlocks retained duplicated descriptors.
Five System and four Current tests cover mutable contents/firmware, no-source-payload admission,
ownership/revocation, inactive/damaged/foreign/missing receipts, namespace replacement, modes,
links/capacity and same-size inode substitution. Metadata fixtures are not usable guest captures.
The signed core probe admits real modified generations, checks fresh zero RAM/controller queues,
executes both CPUs, verifies current/new virtio bytes and reopens after complete ownership teardown.
Ninety-one targeted core/host/model tests and strict all-target lint pass. Full signed vmstate
checks verify three independent current-generation boots per original/two fresh-process readers
and unchanged source-package inventories. Signed direct/registry restore checks also pass with
HGO1 enforced. Installed recovery, rollback and viewers remain open; signed synthetic configured Start/reset
verification is described below.


Private StartGeneration/GenerationStarted now connects System admission to the owned worker and
trusted Sessions::start_generation. It requires matching Active HRR1, current mutable disk/EFI,
independently rebuilt hardware fingerprint and exact topology. The fresh runtime holds every CPU
before acknowledging; only original-policy-bound explicit RestoreResume releases it. It does not
open original disk/EFI paths or replay checkpoint CPU/RAM/device state. Cold resume retains the
policy commit guard through acknowledgement without rewriting Active. Original namespace and
policy checks scope startup, publication, cancellation and reaping. Startup remains private;
agent dispatch rejects StartGeneration and other host-path-bearing startup commands.
Five new session tests cover successful lifecycle/unchanged Active, startup cancellation/revocation,
cancelled resume ownership through acknowledgement and actual exit, configuration/receipt
substitution, and rejection of an Attempted selection before worker creation. Expanded model/wire
tests require exact boot-kind receipts, canonical identity/configuration and original policy.
Seventy-six primary native/model/restore tests and strict model/machine/engine all-target lint pass.
Signed diagnostics cold-boot with original disk/EFI paths absent, recapture current firmware under
the original VM namespace, preserve Active and original data, reject damaged HGO1 without fallback,
and restart after explicit fixture recovery. This synthetic guest does not prove installed Windows.
Automatic rollback, installed/open-app recovery and app/viewer/MCP restore remain unfinished;
signed synthetic configured Start/reset verification is described below.


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

Windows listing reads restore intent only while holding the operation lease and without a foreign
runtime owner. Prepared/Attempted or malformed/unsafe HRR records report Recovery required;
invalid records remain untouched and do not hide other rows. Active metadata with a system-boot
setup phase reports restored intent, without validating generation payloads or claiming guest
readiness. Owned Running/Paused state remains authoritative so lifecycle controls stay available.
Original actor authorization is rechecked before row publication. Five recovery-listing tests and
five existing record tests pass; strict engine all-target lint passes. Automatic rollback and
crash recovery remain pending.

Journal::succeed and Owner::succeed provide trusted successor selection under stopped-runtime
ownership. Exact expected HRR, matching VM/configuration, Prepared next phase, fresh generation
and previous-generation binding are required. Candidate receipt validation rejects an existing
HGA or archived candidate. The predecessor HRR is preserved at restore<generation>.hrr using
sealed temporary publication with exclusive rename and directory sync before replacing current
HRR. Existing exact history supports retry; conflicting or unsafe history rejects without
overwrite. Owner checks the pinned VM namespace around journal work; original policy exclusion
spans archival/replacement. Six tests pass, including process exit after archival and retry with
reacquired operation/runtime ownership. This does not replace component admission or connect
successor preparation, rollback, crash-recovery UI or agent endpoints.

restore::prepare_successor now accepts an owned stopped runtime and exact expected HRR, and uses
the production blocking preparation owner/cancellation path. It validates rebuilt configuration,
source/prior component schemas and fresh generation resources, then succeeds the selection and
marks it Attempted. Existing generation resources remain retained. The signed automatic controller
profile now performs prior-checkpoint recovery through this API and held registry startup, verifies
fresh generation identity, Active predecessor linkage, exact history, original disk/EFI and viewer
watch continuity. Three signed profiles and 25 targeted tests pass; strict engine all-target lint
passes. The fixture is synthetic and successful explicit recovery does not prove automatic rollback
or installed Windows application recovery. Rebuilding hardware still opens the original disk;
original-path-independent successor admission and automatic recovery routing remain pending.

Successor preparation now uses runtime::hardware::checkpoint with a pinned Package::inspect result
and the exact predecessor configuration. Checkpoint workers use the same path against host-selected
Attempted HRR. It never opens original mutable disk/EFI paths: checked HDB1/HDS1 metadata creates an
anonymous sparse controller prototype bounded to 2 TiB with matching disk identity/writable mode.
Independent reconstructed fingerprint/layout checks precede full checkpoint/image admission and
replacement with owned recovered disks before execution. Initial host preparation still validates
original hardware. The signed successor probe removes both original paths before preparation and
verifies fresh Active recovery, retained original contents/history and viewer continuity. Three
signed profiles, 44 targeted tests and strict model/machine/engine lint pass. Temporary prototypes
must never be used for ordinary guest execution. This does not finish automatic rollback,
installed Windows/open-application recovery or UI/MCP recovery routing.

Sessions::start_recoverable now wraps prepared restore handoff with one automatic prior-state
recovery attempt; restore_snapshot uses it after preparation. Original policy, pinned namespace,
session epoch and exact Attempted/Active failed selection bind recovery. A bounded retry acquires
operation/runtime ownership only when the failed worker releases it; retained/quarantined ownership
rejects recovery. A fresh successor uses the failed receipt's rollback checkpoint as source and
keeps failed resources/history. Success still returns the original restore error with an explicit
recovered-prior outcome; recovery failure retains data and never recursively retries. Cancellation,
off/on revocation, changed epoch/selection and invalid prepared receipts cannot reauthorize it.
Four new tests and 26 targeted tests pass; strict model/machine/engine lint passes. The signed
automatic profile injects one pre-worker admission rejection via a diagnostic forwarding wrapper,
then validates real recovered hardware, fresh Active identity, history and existing viewer watch
without original mutable paths. Successful forwarding waits for real child exit; this wrapper is
not evidence of cancellation safety for delegated workers. Preparation failures, later guest
failures, host-crash recovery, installed Windows/open-app rollback and UI/MCP recovery remain open.

restore_snapshot now admits an exact Active predecessor only when its configuration matches the
owned running/paused source. The captured original selection is checked alongside original policy
and session generation during capture/stop. Job preparation transfers the retained operation lease
into prepare_successor; initial restores keep prepare. Every repeat captures a fresh prior checkpoint
and creates a fresh generation, preserving previous resources/history rather than replaying Active
state. The signed automatic profile verifies two consecutive agent transactions, distinct prior and
generation ids, exact predecessor archive/linkage and original viewer watch, then completes prior
recovery and admission-failure rollback with original mutable paths absent. Two new tests verify
preflight rejection and cancelled repeated capture; all 28 targeted tests and strict
model/machine/engine lint pass. Stopped-source restore, preparation-failure rollback, host-crash
recovery, installed Windows/open-app recovery and UI/MCP controls still require work.

Current-generation Gate permits SnapshotStart while cold hardware remains held; checkpoint Gate
continues to reject it before activation. Ordinary Resume/Input/Release/FinishDeployment remain
blocked in either gate. Cold snapshot start changes the initial 30-second deadline to the fixed
admission-time five-minute capture deadline, never an indefinitely sliding extension. The signed
held capture probe publishes and fully decodes current disk/EFI with zero unused RAM; after explicit
resume, a subsequent capture verifies firmware wrote its marker there. Original mutable paths are
absent. Device-tree RAM initially caused a diagnostic false failure; the corrected marker uses RAM
at 0x40100000, outside the DTB and ACPI area. Signed registry/current and direct checkpoint profiles,
22 targeted tests and strict model/machine/engine lint pass. This is a stopped-prior capture core
foundation; the subsequent stopped-source transaction below extends it. Original-disk cold hold
and UI/MCP restore remain pending.

Sessions::restore_stopped now owns an Active generation through held cold startup, fresh named
prior capture/publication, cleanup and actual worker exit, then transfers the same operation/runtime
owner into successor preparation and recoverable activation. Cancellation and the original policy
apply throughout; unverified reaping/scope cleanup retains ownership instead of allowing reuse.
The normal restore_snapshot facade routes idle/stopped sessions through verified bundled assets
and saved configuration, transferring the existing operation lease with the caller's original
policy and requiring exclusive runtime ownership. It never accepts agent boot paths. Process-backed tests cover cancelled startup/capture,
off/on revocation, stale facade policy and competing runtime ownership. The signed automatic probe
exercises the trusted stopped transaction with original disk/EFI paths absent, unique prior and
generation identities, preserved history and viewer-watch continuity. It does not yet exercise
the normal configured facade successfully or an installed Windows desktop. Original-disk stopped
restore, preparation-failure rollback, crash recovery and UI/MCP restore remain unfinished.

Snapshot package writes now seek over bounded all-zero chunks and set the complete logical length
before sealing/syncing. Checksums still cover every logical byte and the package format is unchanged.
A filesystem check verifies fragmented mixed writes, sparse allocation, exact length/private mode,
reopen and full checked-reader byte equality. Existing corruption, authorization, publication and
inventory tests pass. HRM1 already omits completely zero 64 KiB memory chunks; this package change
only sparsifies zero regions actually emitted by component encoders. Populated-memory pressure and
full logical disk hashing remain separate performance gates. A configured diagnostic profile uses
a private app-shaped bundle, production-size 1 GiB RAM/64 GiB disk and the normal stopped-restore
facade. Its synthetic installation receipt admits only the probe and does not prove Windows setup.

Profiling the full-size configured probe found policy JSON parsed directly from File through
serde_json's byte reader, issuing a read syscall for each byte on every bounded disk-hash check.
Policy::check now creates a fresh BufReader around the same size-limited no-follow file. Check
frequency, original generation binding, runtime/identity validation and permanent shared failure
remain unchanged; records are never cached across calls. A new trailing-buffer test verifies late
garbage rejects and invalidates cloned checks after the record is repaired. All 45 targeted policy,
controller/catalog/remote and restoration tests plus strict model/machine/engine lint pass. Logical
disk hashing and successful full-size configured restore remain separate verification gates.

HPA1 now has a native Restore operation carrying only the saved snapshot identity and original
agent generation. The service calls the owned restore facade, rejects pipelining/disconnects,
and cancels without releasing the transaction's retained cleanup owners. Client/server recheck
original policy; the reply contains bounded canonical source/prior/generation identities, never
boot paths. Requests have 910-second service/915-second client limits; expiry may leave durable
recovery state and does not authorize replacement/retry. Host and MCP vm.restore route Windows
through this service, preserving explicit native/migration rejection and leaving other guest
offline behavior intact. Four socket tests verify missing/stale policy, invalid source, disconnect,
pipelining, off/on revocation and boot-path schema rejection. A MCP cancellation test reaches an
external owner and verifies source recovery/cleanup exclusion and access rejection. All 28 targeted
engine/MCP tests and strict model/machine/engine/host/MCP lint pass. Signed synthetic profiles verify
successful HPA1 and real MCP stdio restores, fresh receipts, original viewer watches, subsequent
stopped-generation/prior recovery and admission-failure rollback. This does not establish installed
Windows/open-app recovery, full-size performance, original-disk stopped restore or UI recovery.

New snapshot disk records use HDS2, binding logical size and ordered nonzero 64 KiB chunk hashes.
Allocated zeros and holes hash identically; extent queries skip holes with full-read fallback when
unsupported, retain bounded authorization checks and restore descriptor cursors. HDS1 remains
readable through its original full-file hash. Prepared validation compares exact bytes outside
mutual holes and rechecks source size and private namespace. An independent vector, allocation
equivalence, sparse 64 GiB capture/load/comparison, writes into old holes, cursor preservation and
legacy/unknown-version checks pass. All 94 targeted machine/engine/MCP tests and strict five-crate
lint pass; five existing cases remain ignored. A signed synthetic 1 GiB RAM/64 GiB sparse-disk
profile now passes repeated running restore, normal configured stopped restore, explicit prior
recovery and automatic admission-failure rollback with original mutable paths absent during recovery.
A second full-size signed profile passes real MCP stdio/host/HPA1 restore and all subsequent recovery
checks. The older full-scan run ended with restore/rollback worker timeouts; its fixture is preserved
and HDS1 verification remains a full scan. This supersedes the earlier unverified configured-facade
status. Populated Windows/open-application recovery, preparation-failure rollback, crash/UI recovery
and original-disk stopped restore remain unfinished.

Running restore now performs a bounded component-body integrity precheck before pause/prior capture.
Package::verify_components streams at most 64 KiB, checks the complete body digest against the
manifest-bound footer and preserves authorization checkpoints. The owned restore job retains
session/operation guards through blocking verification, including cancellation. Three machine tests
cover all component bodies, multi-chunk data, framing/permissions and every observed revocation
checkpoint. A process-backed engine check covers body/footer damage, truncation and trailing bytes
without pause, capture, stop, new selection or mutation of damaged saved data. All 48 targeted
machine/engine/MCP checks and strict five-crate lint pass. A signed 1 GiB RAM/64 GiB sparse-disk
agent profile verifies damaged-body rejection while retaining the running VM/inventory, then real
MCP restore, repeated/stopped/prior recovery and automatic admission-failure rollback. This precheck
is not schema admission or separate disk-image verification and does not freeze files. Full validation
before stop is a separate step; preparation-failure rollback remains open.

Running restore now fully decodes the target and fresh prior checkpoint before source stop through
hardware::checkpoint and checkpoint::load. Rebuilt hardware must match the owned runtime's independent
configuration/layout; image checksums and all component decoders run under original authorization,
generation/namespace checks and retained operation/session owners. Validation failure retains the
published named prior checkpoint and resumes only the restore's own source pause. Already-paused VMs
remain paused. The former metadata-only post-capture stop fixture is now an invalid-hardware test
covering running/paused preservation; cancellation through actual exit remains covered on the held
stopped-source path. All 37 targeted engine/MCP checks and strict five-crate lint pass. A signed
1 GiB RAM/64 GiB sparse-disk agent profile rejects a checksummed invalid schema and changed captured
disk before stop, retains both new prior checkpoints and resumes the source; it then passes real
MCP restore, repeated/stopped/prior recovery and admission-failure rollback with full target/prior
decoding enabled. This does not freeze mutable saved files or close late-change/preparation-failure
rollback, installed Windows application recovery, crash recovery or the other release gates.

restore::prepare_retained returns Preparation { owner, result } through the retained blocking task.
The synchronous stage borrows Owner and returns boot/restore data, so ordinary failures preserve
the same operation/runtime/journal leases for an authorized caller. Existing prepare and
prepare_successor wrappers still drop ownership when returning an ordinary error. Dropped callers
cancel and retain blocking ownership through actual work; lost task completion remains an explicit
error rather than a recovery grant. Three regressions verify operation/runtime exclusion, unchanged
predecessor selection and original-policy off/on invalidation. All 30 targeted engine checks and
strict five-crate lint pass. The signed 1 GiB RAM/64 GiB sparse-disk profile verifies failed preparation
retains all three leases and unchanged selection, then continues through prior preparation/start using
the same owner without original mutable paths. The recovery path below supersedes the earlier
diagnostic that repaired its target before continuation. The full MCP/repeated/stopped/prior
and admission-failure rollback profile passes. Automatic preparation-failure recovery now builds
on this ownership path as described below.

Running restore now routes ordinary post-stop preparation failure through recover_preparation under
the same Owner. Original policy, caller cancellation, a bounded deadline and the original registry
generation remain checked. Recovery accepts the exact predecessor or this attempt's Prepared/
Attempted target/prior/configuration/predecessor binding, rejects unrelated selection, fully decodes
the valid prior using independently bound hardware and publishes a separate "Before recovery"
checkpoint. Recovery generation admission uses prior/backup and never requires the damaged target
to validate. Failed history/data remain preserved. Startup uses one nonrecursive attempt and reports
the requested restore failure even after successful prior recovery. Cancellation/off-on revocation
and unrelated/Active selection rejection are tested before backup; all 39 targeted engine/MCP checks
and strict five-crate lint pass. A signed 1 GiB RAM/64 GiB sparse-disk HPA1 probe observes source stop,
then corrupts the already-validated target; automatic prior startup succeeds with that target still
corrupt and a fresh independent backup, original failure response and retained runtime ownership.
The trusted retained-owner path also recovers while the target stays corrupt without original mutable
disk/EFI paths. Real MCP, repeated/stopped/prior recovery and admission-failure rollback pass in the
same signed run. Paused lifecycle support follows below; crash recovery, installed Windows/open-app
checks and other gates remain open.

start_recoverable now routes failed startup through recover_preparation with a fresh independent
backup of the valid prior. It still waits for verified cleanup/ownership release and reacquires only
under the original policy, VM namespace and unchanged registry epoch. Exact Attempted/original Active
selection is checked before recovery; the same epoch, original policy and a bounded deadline apply
through backup and preparation. The damaged target need not validate; startup recovery is one
nonrecursive attempt and keeps the original requested error. A process-backed changed-selection
cleanup regression verifies no overwrite, new snapshots/history or second boot. All 33 targeted
engine/MCP tests and strict five-crate lint pass. The signed rollback diagnostic no longer uses a
rejection/forwarding wrapper; it corrupts the prepared source and launches the actual signed worker.
The full signed 1 GiB RAM/64 GiB sparse-disk run passes, including actual worker rejection, independent
prior backup/startup with the failed target still corrupt, real MCP and preceding recovery checks.
Paused lifecycle support follows below; installed Windows/open-app proof remains open.

Native Windows restore now carries the source's paused state through normal activation and both
automatic recovery paths. The private RestorePause command requires the original restore binding
and durable Active selection, retires the admission gate and retains the CPU hold. Public session
requests cannot forward it, and the host rejects a Running reply to paused activation. Cancellation
still drains and reaps before releasing ownership; off/on policy changes and mismatched handoff
policy reject before dispatch. Initial held recovery now permits fresh capture by retaining the
admitted CPU/PMU/frozen-clock state behind the original recovery barriers while capturing current
memory/devices and preserving relative pending actions. Interrupt delivery waits for actual resume.
The first signed paused check exposed the former held-capture restriction; its failed fixture remains
preserved. Updated signed 1 GiB RAM/64 GiB sparse-disk checks verify paused post-stop preparation
recovery, normal paused restore, unchanged guest RAM through a 31-second hold beyond admission expiry
and changed RAM only after explicit resume. Two independent held captures also exactly retain both
original HCP1 CPU/PMU/clock records and the HSW1 software record. The focused runner then exposed a
diagnostic watch-order error after those checks; it is corrected. Continuing the preserved fixture
through verified configured startup passes real-worker late-corruption recovery in both running and
paused modes, with an independent backup, unchanged failed target/history and no original mutable
paths; the continuation exits successfully. Targeted model/machine/engine tests, MCP contracts and
strict five-crate lint pass. A fresh combined signed rerun, installed Windows/open-app recovery and
the broader release gates remain pending. Low available storage prevents a fresh combined fixture;
failed fixtures and their original diagnostic binaries/logs remain preserved.

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
reboots retain retry intent. Official ESD conversion and unattended deployment are now live
verified. Clean WinPE shutdown now precedes owned installed startup, and the new real run
reaches the signed-in desktop with the formerly damaged driver matching the official image.
Live dedicated viewer/input inspection and guest-tool integration remain pending. Existing prototype instances
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
disabled until their native implementation exists. A signed isolated app verifies installed Windows
sign-in rendering, viewer pause/resume and close/reopen with the same worker. Pointer checks found
upper-half coordinates wrapping negative in the Windows HID path; device axes now advertise 0–32767
and scale the unchanged public 0–65535 input range at enqueue. Live center/lower-corner targeting,
account selection, the guest power menu and guest-initiated shutdown pass after replacement.
The worker exits and the viewer reports Stopped without restarting the guest. Five device tests cover both axes,
edges, center, invalid ranges, buttons and keyboard release. Screenshot replies previously delayed
key releases by roughly 1.3 seconds with the viewer polling. A single owned output thread now
waits on nonblocking pipe readiness independently of CPU callbacks. It retains at most one reply,
keeps serial framing, releases the frame before reporting completion, rejects blocking descriptors,
expires after five seconds without progress and joins after cancellation on owner teardown.
The low-level flush remains bounded to 4 MiB, 256 calls and two milliseconds, returning immediately
on WouldBlock. Seven output tests cover full serial frames, the 64 MiB maximum, stalled-reader
expiry, disconnect, cancellation, invalid payloads and blocking-descriptor rejection; six IPC tests
and all 146 machine tests pass, as does strict machine/diagnostic lint. A signed isolated Windows app
verifies three short keypresses produce three characters and Control+A/Backspace clears them.
The production agent diagnostic with its viewer open measures 20–125 ms acknowledgements and
reveals exactly the dummy string aBc, proving Shift press/release. A separate viewer-input run
reveals that same exact string without replacing it through agent typing. Later captures retain
three characters. Guest-initiated shutdown still drains the stopped event, exits the worker and
shows Stopped after the writer replacement. All 13 native-client/input-control regression tests pass.
After all CPUs acknowledge a new coordinated pause, the owner now delivers its paused callback
immediately instead of waiting another polling tick. A signed two-CPU probe verifies stable RAM,
resume, paused stop/error/panic cleanup and reacquisition. Its callback latency falls from 46 to
25 ms; ten single-CPU samples fall from 24–25 ms to 0.7–1.8 microseconds. Held-paused polling
retains its normal cadence. Live Windows manual pause/resume and viewer close preserve the same
worker after this timing change. This does not prove responsiveness at larger resolutions, broader
modifiers/held-input release or full desktop interaction; those checks remain open.
Synthetic worker checks alone do not prove Windows desktop readiness.
VM and temporary target leases explicitly unlock on owner teardown; closing a file
alone can retain a flock through a duplicated or inherited descriptor. Do not replace
those leases with bare file handles. The download writer retains its inode lock through
pending asynchronous writes and has a different ownership lifetime.
Dropping the final client stops the worker.
Started means allocated hardware, and Deployed means deployment completed; neither
proves a usable desktop. Direct VZ integration, inside-guest transport, guest tools and
running-memory snapshots remain pending.

Native Linux configurations include one virtio socket device. The core owns an optional
guest-initiated listener on port 6200 on the VM queue, with one retained connection, bounded
64 KiB nonblocking I/O, close-on-exec and no SIGPIPE. Original authorization applies before
acceptance and before/after I/O; policy failure closes the connection and clears read bytes
if revoked during a read. Stopped or paused hardware cannot serve transport I/O. A listener
retains runtime ownership, prevents duplicate attachment and blocks queue retirement until
closed. A signed disconnected Linux diagnostic verifies a real UID 1001 socket exchange,
I/O bounds, revocation/off-on rejection, ownership lifetime and retirement exclusion. This
is a core transport foundation, not an agent endpoint. Guest service provisioning, job/file
framing, app/MCP integration, Windows transport and macOS socket configuration remain pending.
Automated native VZ viewer Shift/Control chords arrive without modifiers; physical modifier
input is unverified.

The `guest` crate now provides a Linux job runner, authenticated command dispatcher and
Linux socket daemon. These are not installed or connected to Hopper's agent endpoints.
The shared guest contract uses literal executable/argument lists, explicit normal/admin roles,
OS-specific paths and 1–600 second timeouts. Jobs return Starting with no identity; successful
child admission records its verified launch UID/GID/groups. Normal jobs use the actual hopper
account; admin jobs use root. Protected account databases supply home/groups, the child joins
its owned cgroup before reducing privilege and changing directory, and no inherited environment
or stdin is passed. Startup cancellation takes priority over failed-launch classification.
No-new-privileges and disabled keep-caps prevent implicit normal-user elevation. A private
cgroup-v2 root has one flock owner; pinned directory handles scope creation, kill and cleanup.
At most two jobs run, history retains 16, and each output stream retains 64 KiB while draining
excess bytes. Cancellation, timeout and manager drop kill the cgroup tree, including detached
sessions. Cleanup is bounded and completion/capacity release wait for reaping, empty groups
and verified removal; failed cleanup remains unfinished and holds its slot. Unknown prior
groups reject startup without deletion. A signed disconnected Linux diagnostic verifies real
UID 1001/0 execution, groups, zero normal capabilities, setuid rejection, literal arguments,
startup errors, bounded outputs, capacity/history, nested cleanup, detached-descendant kill,
timeout, drop/reopen and unknown-group preservation. Linux musl Clippy is checked explicitly
in CI; local cross/host lint and model tests pass. Installed authenticated socket transport,
service provisioning, app/MCP jobs and non-Linux runners remain pending.
The shared `model::guest::wire` HPG1 codec frames start/poll/cancel requests and job/error
replies. It accepts 64 KiB I/O chunks, bounds requests to 256 KiB and replies to 1 MiB,
rejects pipelining, validates commands and job/output fields and permanently closes after
framing/schema errors. Sequence identifiers are nonzero, but replay/session authorization
is not implemented by the plain codec. Five pure tests cover fragmentation, malformed/oversized
messages, schema rejection, escaped arguments and full binary outputs. Guest errors are
bounded to 512 UTF-8 bytes. The guest diagnostic now round-trips every polled reply and adds
a Unicode launch failure; these additions have not been live-verified.
The separate authentication layer uses HMAC-SHA256 with a per-boot 32-byte secret, fresh
host/guest nonces and a three-message mutual handshake. Proofs and directional frame keys
bind the VM identity, runtime generation, guest OS and original policy generation. Guest
handshakes require trusted boot identity; policy is supplied by the authenticated host.
Serial request/reply sequences reject replay, reflection and pipelining; authentication,
framing or sequence errors close the session and erase owned keys. Authenticated incremental
reads retain the same chunk/frame bounds and verify the MAC before decoding job fields.
The Linux service dispatcher only accepts a completed guest session and routes start/poll/
cancel to the production runner. Disconnect, malformed input and dispatcher drop cancel its
jobs and retain cgroup ownership through cleanup. The updated signed diagnostic includes
authenticated UID 1001/root jobs and forged-frame process cleanup, but has not run live.
Boot-secret provisioning, daemon installation, original host-policy
checks around dispatch/publication, app/MCP routing and installed service verification remain
pending. Message authentication does not encrypt the dedicated VM transport.
The `hopperguest` binary requires root and accepts only its fixed `--bootstrap` mode or
ordinary no-argument service startup. The service reads an exact 61-byte
HGB1 boot document from `/run/hopper/boot` through a pinned private root-owned directory.
The directory must be 0700; the file must be regular, root-owned, single-link and 0400.
No-follow, close-on-exec and nonblocking opens precede bounded reading. The shared material
codec rejects truncated/extended documents, unknown OS, nil identity, zero generation and
zero keys. Its three pure tests pass. The daemon connects only to host CID 2, port 6200,
and verifies its local CID belongs to a guest before any cgroup preparation or command work.
Socket I/O is nonblocking, close-on-exec, capped at 64 KiB per syscall and uses no-SIGPIPE
writes. Connect and partial-frame/reply deadlines are five seconds; mutual authentication
has ten seconds. Idle polls and stop checks use at most 100 ms; reconnect delays are two
seconds. SIGTERM/SIGINT cancel work. Connection teardown waits for confirmed job cleanup
before reconnect or normal process exit; failed cleanup remains owned and unfinished.
The systemd unit is included in new installer seeds. ARM64 musl daemon/diagnostic builds and
host/cross lint pass; actual socket exchange, signal cleanup, private-file checks and service
activation remain live-unverified.
The portable `machine::guest` host connection drives the mutual handshake and serial job
requests through a nonblocking transport. Each tick performs at most one bounded read/write;
waiting for a daemon is limited to 180 seconds, authentication to ten seconds and a request/
reply to five seconds. Original authorization is checked before/after work and again before/
after taking a cached reply. Busy requests do not replace an in-flight packet; malformed
replies, EOF, unsolicited idle bytes, deadlines and revocation discard buffered data, close
keys and drop the transport owner. The VZ adapter remains on its VM queue. Socket runtime
ownership now lives in delegate state so queued SDK callbacks retain the same ownership as
their hardware. `Owner::guest_commands` binds supplied boot material to its owned Linux id;
callers must supply a check capturing the original runtime/policy generations and pump the
connection on that queue. Five portable peer tests verify fragmentation, serial literal
commands, deadlines, revocation during reads/publication, EOF/corruption and owner release.
The command client's queue-thread compile-fail check passes. Host/cross lint and model checks
are local evidence, not proof of a live VZ/daemon exchange. Live boot-secret delivery,
external MCP verification and live callback ownership checks remain unfinished.
The latest directory-ordering and startup-cancellation changes cross-compile and pass lint/model
checks; their updated live diagnostic is unverified because the current execution profile reports
Virtualization.framework unavailable. Earlier signed job checks passed before those changes.
Linux provisioning exposes `provision::with_tools` for public ARM64 daemon/service files.
It preserves accounts, files and startup commands, adds root-owned executable/unit entries,
enables without starting, and rejects malformed plans, conflicting paths and oversized input.
NoCloud user data now permits 2 MiB for base64 ELF payloads; metadata remains 4 KiB.
Independent ISO extraction verifies exact binary bytes and preserved configuration. Do not
place boot secrets on guest-readable installer media. Automatic Ubuntu preparation now loads
and validates the bundled tool bytes before installer staging and adds them to its seed.
The enabled unit still requires private per-boot credentials before it can start.
`scripts/build/guest.sh` builds the static ARM64 musl daemon and packages it separately in
`native/build/guesttools`, preserving existing kernel media. Bundling rebuilds the package and
checks its copied resource manifest. Validation checks exact inventory, regular files, bounds,
SHA-256, ARM64 ELF headers, static linkage and executable entry segments. The actual package,
two malformed/tampered-asset tests, six provisioning tests and engine Clippy pass locally.
The runtime loader requires an exact manifest/inventory, bounded no-follow regular-file
reads, matching sizes/hashes, static ARM64 ELF and the service text from this build. App and
sidecar resolution never falls back to development assets. Four loader tests pass locally,
including independent extraction of the actual daemon from production seed media. CI now
builds the package and runs that explicit integration test, but the new workflow is unrun.
Full signed bundling, dependency license inventory, protected boot credential delivery and
installed service activation remain unfinished or unverified.
`machine::vz::bootstrap` prepares exact HGB1 input on an anonymous pipe, with close-on-exec
flags and original authorization before/after preparation. The consuming hardware factory
uses a named `hopper.boot` virtio console port marked non-console, with guest output discarded
to `/dev/null`. It preserves diagnostic serial output. Framework handles retain duplicated
descriptors; configured hardware remembers the boot identity/generation. Command admission
rejects a different bound boot, and later starts require new hardware because input is
single-use. Two exact-material/EOF/flags/policy tests and the existing five peer tests pass.
Linux system admission now uses the private factory. Live SDK/unprivileged-access
verification remains unverified. Do not infer guest-device secrecy from host pipe tests.
The Linux bootstrap reader discovers at most 64 literal vport devices through verified
sysfs, selects the unique `hopper.boot` name and matches its reported major/minor to an
opened no-follow character device. It requires root ownership and mode 0600 before every
bounded read and after completion. Waiting is bounded to 180 seconds with stop checks;
the exact 61-byte temporary buffer is wiped on drop. Runtime publication pins root-owned
tmpfs `/run`, opens or creates private 0700 `hopper`, exclusively locks the directory and
preserves unknown/interrupted state. A staged 0400 file is synced and published with
renameat2 no-replace. Existing credentials must pass the service's complete file checks.
Systemd runs `--bootstrap` before the daemon; new seeds install a root-only udev rule with
uaccess/seat tags removed. Two pure device-name/number tests, rebuilt static ARM64 package,
ten provisioning/resource tests and host/cross Clippy pass locally. Actual sysfs/device
permissions, ordinary-user rejection, publication/cancellation, systemd startup and live
host-to-guest key delivery remain unverified.
Linux system admission generates fresh random material bound to the owner's next hardware
generation and inserts through an identity/generation check. Installer hardware keeps its
existing path. The VM retains private material for command connections; `Owner::commands`
creates a fresh handshake under the supplied original policy without exposing the key.
`Material::handshake` makes an owned, zeroized session key copy internally, preserving its
source for reconnect. A pure test independently authenticates repeated policy-scoped sessions
with fresh nonces. Library Start retires stopped, previously started private-channel hardware
through the existing viewer/runtime exclusion and prepares a fresh system boot. Direct Start
cannot reuse consumed input. Local app check, core/app Clippy and protocol/provisioning tests
pass; Metal shader compilation needed an explicit writable temporary module-cache path.
Actual SDK admission, GUI restart, key receipt, guest service activation and command execution
remain live-unverified. External MCP verification remains unfinished.
The VZ owner now drives at most 16 authenticated guest links at 50 ms intervals, with one
pending request per VM and persistent connections between job calls. Its existing app pump
stays active independently of viewers. Both session and request checks run around transport
work and reply publication; hardware generation is verified on dispatch and each drive.
Queued cancellation skips dispatch; cancellation after connection admission closes that
VM's link, cancelling the session's guest jobs. Pause/stop clear links. Revocation, protocol
errors and transport failure discard buffered state and release request checks. Host exposes
`virtual_guest_request` through an agent-authorized native Linux service. The session check
holds no per-request operation lease; request `.commands` leases exclude competing commands
without blocking lifecycle `.operation` leases. Three queue admission/cancellation/policy tests,
five portable protocol tests and strict app/core Clippy pass locally. Actual installed jobs,
persistent polling, concurrent Stop and lease recovery remain live-unverified. MCP socket
verification, inside-guest access, non-Linux runners and full guest command QA remain unfinished.
The existing same-user HPA1 endpoint now routes native Linux guest job bodies to the app's
registered VZ service. Registration retains a queue client, not the main-queue VM owner.
Headers stay at 4 KiB; HPG1 bodies are independently bounded to 256 KiB requests/1 MiB replies,
decoded with at most 64 KiB heap chunks. Authorization precedes body reads and is rechecked
through dispatch and output delivery. Disconnect/pipelining cancels the queued operation.
MCP adds `vm.job_start`, `vm.job_poll` and `vm.job_cancel`, with explicit normal/administrator
roles and literal executable/argument lists. The original counter from initial MCP admission
is carried through socket and queue service; it must never be refreshed after an off/on toggle.
Three framing/original-policy tests, two MCP catalogue/validation tests and strict app/MCP
Clippy pass locally. The new socket tests, existing control suite and existing MCP machine
regression fail at Unix socket binding with Operation not permitted in this execution profile.
They remain unverified required checks. A large async stack buffer caused an earlier stack
overflow; bounded heap storage corrected it. End-to-end socket/guest execution, installed
service, secret delivery, process cleanup and live lifecycle races remain unverified.
The disposable `vzcommands` example now combines the production daemon, private bootstrap
factory and owned queue request API. `scripts/check/commands.py` packages the daemon into
verified Alpine initramfs media, mounts private runtime tmpfs/cgroup2, loads virtio console/
socket modules from a matching read-only modloop and checks ordinary-user rejection of the
key file/device before starting the daemon. Host checks require UID 1001/0 output and launch
identity, cancelled-job completion, stopped hardware and eventual callback ownership release.
The probe has no NIC, shares or speaker and uses only private temporary disks; it cannot
establish external MCP socket routing or installed Ubuntu/systemd readiness. Production daemon
bytes were independently recovered from the prepared archive; source ISO SHA-256 was rechecked
before extracting modloop. Build/signature and strict machine Clippy pass locally. The current
signed live attempt returns Virtualization is unavailable on this host. All probe guest checks
remain unverified; do not treat packaging or compilation as their success.

## What this is

Native Linux streaming uploads now route through authenticated actions BeginUpload,
UploadChunk, UploadStatus and CommitUpload, exposed as vm.upload_begin/chunk/status/commit.
Specs bound size to 1 GiB and time to 600 seconds. Private AF_UNIX stream stdin carries
binary checksum/header data; payload and digest stay out of argv. Nonblocking sends
without SIGPIPE use a bounded socket buffer and report accepted offsets under backpressure.
The selected-role helper requires all declared bytes, an explicit marker, EOF and matching
SHA-256 before anonymous no-replace publication. Job cancellation and service teardown close
producer channels; EOF without a marker never commits even after all payload bytes arrive.
Offsets are queued-byte acknowledgements, not durable receipts or cross-session resume.
Commit state is not publication proof; require successful supervised job completion and
its Uploaded receipt. Cancellation/failure after commit may leave the completed destination.
The nested job and transfer offsets are bounded and validated on the wire. Original VM
agent policy remains required through the existing transport. Contract/authentication/MCP
and packet tests pass, and host/musl Clippy pass. Linux producer/receiver tests compile but
have not executed. The signed diagnostic includes normal/admin multi-chunk upload and
stamp-checked readback. Installed service updates, live upload/cancel/revocation verification
and Windows/macOS implementations remain pending.
Service teardown marks all jobs cancelled before closing producer channels. Upload chunk
and uncommitted commit operations check cancellation/done intent while holding the producer
gate, preventing mutation during the gap before cleanup. Already-committed status remains
queryable without implying rollback. Four Linux-only supervisor cancellation/finished-state
tests compile but have not run. Signed diagnostic source now also checks fully queued
uncommitted cancellation, commit rejection after cancellation, checksum failure and
destination absence; these new cases remain unexecuted.

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
- **`guest`** — guest-only services; the Linux job runner owns cgroup supervision and
  identity changes inside the VM. It must never execute guest requests on the host.
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
