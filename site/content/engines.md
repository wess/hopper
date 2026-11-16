---
title: Engines
group: Start
order: 2
summary: Hopper Engine on macOS, Docker or Podman on Linux, and any compatible Engine API endpoint.
---

Hopper discovers the engines that make sense on the current machine, shows them
separately, and says which one answered. A saved choice stays selected even
while it is stopped so an outage never silently moves workloads elsewhere.

## macOS — Hopper Engine

Hopper Engine is the default on Apple silicon Macs. It is a private, headless
Linux VM running rootful Docker. A bundled Lima helper drives Apple's
Virtualization framework; users do not install Lima, Docker, or a separate
daemon first.

The VM lives under `~/.hopper/engine/lima`, isolated from other Lima and Colima
instances. Hopper owns its lifecycle, Docker readiness checks, CPU and memory
budget, file shares, port forwarding, daemon configuration, and disk reclaim.
The Docker socket is published only to Hopper's private host path.

The first start downloads the pinned Linux base, prepares the disk, starts
Docker, and waits for the daemon to answer. VM boot alone is not considered
ready. Later launches reuse the disk and return much faster.

## macOS — Apple Containers

[Apple's `container`](https://github.com/apple/container) remains an optional
backend on supported Apple silicon Macs. Hopper can install Apple's signed
package, start its services, and drive the CLI. Every container is a lightweight
VM and the runtime publishes no Docker socket.

That missing socket is the important boundary. Hopper can manage the runtime,
but the Docker CLI, Engine API integrations, live stats, file browsing, terminal,
healthchecks, pause, rename, and post-create resource updates are unavailable.
The UI hides those controls rather than drawing buttons that always fail.

## Linux — Docker or Podman

Hopper attaches to what the host already runs. Rootless sockets come before
system sockets:

1. `$XDG_RUNTIME_DIR/podman/podman.sock`
2. `$XDG_RUNTIME_DIR/docker.sock`
3. `/var/run/docker.sock`
4. `/run/podman/podman.sock`

`DOCKER_HOST` wins when set; `CONTAINER_HOST` is accepted when `DOCKER_HOST` is
unset. Podman's Docker-compatible socket uses the same Engine API path.

## Windows — Docker Desktop or Podman

Hopper checks Docker Desktop's `docker_engine` named pipe and Podman's default
machine pipe. They appear as separate choices when both are present. Hopper
attaches to the selected daemon and does not take over its lifecycle.

## Existing and remote engines

Docker Desktop, Colima, Rancher Desktop, Podman, OrbStack, a Unix socket, a
Windows named pipe, or a daemon over TCP/TLS all work through the Engine API.
Hopper resolves the endpoint fresh for each request, so switching providers does
not leave a long-lived client pointed at the old engine.

## Capabilities

| | Hopper Engine / Engine API | Apple Containers |
|---|---|---|
| Start, stop, restart | yes | yes |
| Logs and inspect | yes | yes |
| Live stats, files, terminal | yes | no |
| Pause / unpause | yes | no |
| Rename | yes | no |
| Healthchecks and restart policies | yes | no |
| Post-create CPU / memory update | yes | no |
| Docker CLI and Engine API tools | yes | no |
| Compose stacks | yes | yes, through Hopper's parser |

## Choosing one

Use **Settings → Which engine**, or set `HOPPER_ENGINE`. Common ids are `vm`,
`existing`, `docker`, and `podman`; macOS also offers `apple`, `colima`, and
`rancher` when present.

```sh
HOPPER_ENGINE=vm /Applications/Hopper.app/Contents/MacOS/hopper
```

An explicit choice is honoured even when it cannot run. Hopper reports the
selected engine's real state instead of quietly answering about another one.
