---
title: Settings
group: Use
order: 7
summary: Engine selection and lifecycle, CLI integration, and appearance.
---

## Engine

What the engine that is answering has to say for itself, and — when it is one
Hopper manages — buttons to start and stop it. An engine that is not installed
yet has nothing to start, so those buttons stay off and the install is offered
below instead.

## Which engine

**Automatic** is the default: Apple Containers on a Mac, Docker or Podman on
Linux, and whatever is already running as the fallback. When nothing is running,
automatic lands on the engine Hopper can supply rather than reporting a missing
Docker — Docker is not a requirement on macOS.

Pin one instead and Hopper stays on it. That includes an engine that is not
installed yet: pinning the one you are moving *to* is the point, and its status
then says what is left to do about it. A Mac without Apple's `container` is also
offered the install here — reachable while Docker is still connected and
working, which is the only moment someone happy on Docker Desktop would think
to look.

`HOPPER_ENGINE` in the environment overrides the saved setting, which is handy
for testing without changing what is saved.

## Docker CLI

On an Engine API engine, the `DOCKER_HOST` line that points `docker` and
`docker compose` at whatever Hopper is showing.

On Apple Containers there is no such line. That runtime publishes no Docker
socket, so `docker` cannot be pointed at it at all — Apple's own `container`
command drives the same runtime Hopper is showing.

## Appearance

Dark and light, following the system by default.

## What is not here any more

Hopper used to offer a **Resources** section — CPUs, memory, disk — and a
**File sharing** list. Both belonged to the Linux VM it used to run.

Apple sizes each container's VM when it runs it, and bind-mounts host paths
directly, so a global budget has nothing to apply to and there is no share list
to keep. Numbers that change nothing are worse than no numbers: they invite you
to tune them and then wonder why nothing happened. Both sections are gone.

## Where settings live

JSON under `~/.hopper/`, written atomically, with a corrupt file backed up
rather than silently replaced. Credentials go to the OS keychain instead, one
single-line JSON blob per key — the macOS keychain corrupts values containing a
newline.

`HOPPER_DIR` overrides the root, which is what the test suite uses.
