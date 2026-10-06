# Hopper

## Platform

Native desktop app. Supported releases target Apple silicon Macs running macOS 26 or newer.

## Users and purpose

Developers and agents running containers and desktop virtual machines. Hopper replaces Docker Desktop and adds viewable Linux, macOS, and Windows guests.

## Product constraints

The container engine and user VMs have separate lifecycles and persistent storage. Stopping a user VM must not stop containers.

Each VM opens in a dedicated viewer window. Agent access is enabled by default for new VMs and can be disabled per VM. Clones inherit the source's setting. Agents should have guest commands, screen capture, input, file transfer, and snapshot recovery, without gaining access to arbitrary host files or host input.

Keep the existing native interface and visual system. Guest support must describe actual backend capabilities. Windows and macOS guest paths currently depend on experimental Lima functionality; Windows requires a Microsoft installer and additional runtime dependencies.
