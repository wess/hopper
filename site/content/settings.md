---
title: Settings
group: Use
order: 7
summary: Engine selection and lifecycle, CLI integration, and appearance.
---

## Engine

Which engine to prefer, whether to start it automatically, and buttons to start
and stop it when it is one Hopper manages.

The picker lists only what this machine can run. On a Mac without Apple's
`container`, the Apple option explains what it needs rather than failing when
selected.

`HOPPER_ENGINE` in the environment overrides this setting, which is handy for
testing without changing what is saved.

## Docker CLI

The `DOCKER_HOST` line for the active engine, and an option to expose the bundled
`docker` and `docker compose` binaries on your `PATH`.

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
