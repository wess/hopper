# Hopper

## Platform

Native desktop app. Supported releases target Apple silicon Macs running macOS 26 or newer.

## Users and purpose

Developers and agents running containers and desktop virtual machines. Hopper replaces Docker Desktop and adds viewable Linux, macOS, and Windows guests.

## Product constraints

VM engines and firmware must not require paid third-party licenses or subscriptions.
Redistributed firmware retains its required license notices. Guest OS licensing is separate.

The container engine and user VMs have separate lifecycles and persistent storage. Stopping a user VM must not stop containers.

Each VM opens in a dedicated viewer window. Agent access is enabled by default for new VMs and can be disabled per VM. Clones inherit the source's setting. Agents should have guest commands, screen capture, input, file transfer, and snapshot recovery, without gaining access to arbitrary host files or host input.

Keep the existing native interface and visual system. Guest support must describe actual backend capabilities. All guest OS images download automatically on first start. A local ISO or restore image is an optional fallback.

## Major VM release requirements

These are release gates, not descriptions of the current implementation. Earlier desktop VM
records used Lima and QEMU. Their records and guest disks must remain recoverable during replacement.

- Hopper owns each VM's runtime and dedicated viewer. Linux and macOS use Virtualization.framework directly; Windows ARM64 uses Hypervisor.framework with Hopper's firmware and device integration. The desktop VM runtime must not launch QEMU or delegate lifecycle and viewers to Lima.
- Creation downloads, verifies, and caches official installation media, installs the selected system, provisions the guest account and guest tools, and opens a usable desktop. Progress covers each phase and supports interrupted-download recovery. Windows setup must not require a product key to proceed; Windows activation remains separate.
- Guest tools provide display resizing, clipboard, file drag and drop, named shared folders with explicit access modes, and guest commands. Input and screenshots refer only to the guest display. Host credentials and arbitrary host files must not become accessible through guest control.
- Hardware settings cover CPU, memory, GPU acceleration, and storage. Windows supports accelerated Direct3D 11 and OpenGL graphics. Guest disk capacity and the sparse host storage limit are separate; the storage limit is enforced while the VM runs. Settings state which changes apply immediately and which require restart or shutdown.
- Sharing settings cover folders, clipboard, drag and drop, network disconnection, localhost port forwarding, speakers, and opt-in microphone access. Unsupported capabilities are unavailable with an explanation.
- Agents on the host or inside a guest can connect to a VM through documented configurations. Per-VM access defaults on for new VMs and takes effect immediately when changed. Connections have explicit input ownership, held key/button release, inactivity recovery, and bounded screenshots.
- Guest command execution returns jobs with polling, bounded output, timeouts, and process-tree cancellation. Normal-user and administrator execution are distinct and report the actual execution identity. Guest file transfer and snapshot recovery remain available to authorized agents.
- Independent snapshots preserve disks and, for running guests, memory and open applications. Capture briefly pauses a running VM; restore preserves a recoverable prior state. Clones inherit agent policy and share unchanged disk blocks without sharing mutable VM identity.
- Release evidence includes live installation and first boot for Windows, Linux, and macOS; guest display/input, graphics, sharing, agent operations, snapshot/restore and clone checks; container-engine regression checks; signed app and installer verification; and successful release automation. A CPU probe or green unit suite alone does not satisfy these gates.
