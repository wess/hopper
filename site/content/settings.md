---
title: Settings
group: Use
order: 7
summary: Engine choice, Hopper Engine resources, Docker configuration, CLI tools, and replacement safety.
---

## Engine

The status panel names the selected provider and, for an engine Hopper manages,
offers its supported lifecycle actions. Hopper Engine can be started and stopped
here. External desktop runtimes remain under their own application's control.

## Which engine

**Automatic** prefers Hopper Engine on a supported Mac, the available Docker or
Podman socket on Linux and Windows, and an existing compatible endpoint when one
is already selected. Pinning a provider keeps that choice even while it is
stopped, so an outage cannot silently redirect requests to another daemon.

`HOPPER_ENGINE` overrides the saved choice for testing and automation.

## Hopper Engine resources

Set the private VM's CPU count, memory, and disk budget. File shares decide which
host paths containers can bind-mount. Docker settings accept daemon JSON for
advanced configuration. Stop and start Hopper Engine after changing VM, share,
or daemon settings.

The disk panel reports usage and can reclaim unused VM space. Idle suspend can
stop an empty Hopper Engine after the configured delay; autostart controls
whether Hopper starts it on launch.

These controls appear only for Hopper Engine. Docker Desktop, Podman, Colima,
and remote daemons own their resources elsewhere.

## Coexist or replace

This section discovers other local container applications and keeps replacement
explicit. You can continue using one, run it beside Hopper Engine, import its
workloads, or review removal after backups and verification. Hopper shows
destructive consequences and requires typed confirmation before opening the
vendor removal command in Terminal.

Switching engines and importing never uninstall anything.

## Docker CLI

**Install Docker CLI tools** configures Hopper's bundled Docker, Compose,
Buildx, and credential helper for the current user, then creates or updates the
`hopper` Docker context for Hopper Engine's private socket. It can also repair
links left behind when Docker Desktop was removed.

Apple Containers publishes no Docker socket, so Docker CLI tools cannot target
that optional backend.

## Appearance

Choose light, dark, or follow the system.

## Where settings live

JSON lives under `~/.hopper/` and is written atomically. A corrupt document is
backed up instead of silently discarded. Credentials live in the OS keychain as
single-line JSON values. `HOPPER_DIR` overrides the root for isolated testing.
