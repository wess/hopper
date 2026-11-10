# Hopper

## Platform

Native desktop app. Supported releases target Apple silicon Macs running macOS 26 or newer.

## Users and purpose

Developers and agents running Docker-style containers. Hopper replaces Docker Desktop.

## Product constraints

Hopper is a container tool, not a general virtual machine manager. It does not create,
display or manage desktop guests. The one VM it owns is the headless Linux VM behind
Hopper Engine, which exists only to run the Docker daemon and has no window.

The container engine must not require paid third-party licenses or subscriptions.
Bundled third-party tools retain their required license notices.

Keep the existing native interface and visual system. The UI describes what the active
engine can actually do: a capability a backend lacks is hidden or explained, never
offered as a button that fails.
