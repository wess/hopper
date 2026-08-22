---
title: Engines
group: Start
order: 2
summary: Apple Containers on macOS, Docker or Podman on Linux, and anything that answers the Engine API.
---

Hopper does not have one engine. It has three ways of finding one, tried in
order, and it tells you which answered.

## macOS — Apple Containers

[Apple's `container`](https://github.com/apple/container) ships as part of the
macOS 26 story: every container is its own lightweight virtual machine, images
are OCI, and Apple maintains it. Hopper installs it, starts and stops its
services, and drives it.

This is the default on macOS. If `container` is not on the machine, Hopper
offers to fetch the package **Apple** signed and hands it to the system
installer, which asks you to approve it. Hopper never elevates privileges
itself.

## Linux — Docker or Podman

Whichever you have. Hopper checks rootless sockets before system ones, so a
desktop Podman is not passed over in favour of a stale root daemon:

1. `$XDG_RUNTIME_DIR/podman/podman.sock`
2. `$XDG_RUNTIME_DIR/docker.sock`
3. `/var/run/docker.sock`
4. `/run/podman/podman.sock`

Podman's socket is deliberately Docker-compatible, so nothing else changes.
`DOCKER_HOST`, when set, wins outright — if you are pointing at a remote daemon
you mean it.

## Anywhere — an engine you already run

Docker Desktop, Colima, Rancher Desktop, or a daemon over TCP. This one is
always available and always last, so Hopper stays useful whatever else fails.

## What each engine can do

Apple's runtime is not the Docker Engine API, and Hopper does not pretend
otherwise. It publishes no Docker socket — [the request for one was closed as
not planned](https://github.com/apple/container/issues/636) — so Hopper speaks
to it through the `container` command instead. Some things are simply not there:

| | Engine API | Apple Containers |
|---|---|---|
| Start, stop, restart | yes | yes |
| Logs | yes | yes |
| Pause / unpause | yes | **no** |
| Rename | yes | **no** |
| Change CPU / memory after create | yes | **no** |
| Restart policies | yes | **no** |
| Healthchecks | yes | **no** |
| Live event stream | yes | **no**, Hopper polls |
| Live stats | yes | not yet |
| Shell into a container | yes | not yet |
| Browse the filesystem | yes | not yet |
| Compose | yes | **no** |

Hopper hides what an engine cannot do rather than offering a button that always
fails. On Apple Containers the detail pane shows Logs and Inspect instead of five
tabs where three cannot load, and Stacks leaves the sidebar entirely.

If you need any of it, switch engines under **Settings → Engine**.

## Choosing one yourself

`HOPPER_ENGINE` overrides everything, then the saved setting, then the platform
default:

```sh
HOPPER_ENGINE=existing /Applications/Hopper.app/Contents/MacOS/hopper
```

Valid ids are `apple`, `linux`, and `existing`.
