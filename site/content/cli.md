---
title: Docker CLI
group: Use
order: 6
summary: Point docker and docker compose at the engine Hopper is running.
---

Hopper is a GUI, not a replacement for your terminal. Both should talk to the
same engine.

## With an Engine API engine

Point `DOCKER_HOST` at whatever Hopper reports under **Settings → Docker CLI**:

```sh
export DOCKER_HOST="unix:///var/run/docker.sock"
```

Or make it permanent with a context:

```sh
docker context create hopper --docker "host=unix:///var/run/docker.sock"
docker context use hopper
```

## With Apple Containers

There is nothing to point `DOCKER_HOST` at — Apple publishes no Docker socket.
Use Apple's own CLI, which is installed alongside the runtime:

```sh
container ls
container run --detach --name web --publish 8080:80 nginx
container logs -f web
```

Most Docker muscle memory transfers. The flags that do not have an Apple
equivalent — `--restart`, `--hostname`, `--network=none` — fail loudly rather
than silently doing nothing, which is the right choice.

## What the app bundles

The release ships the Docker CLI and a standalone Compose binary inside
`Hopper.app`, because Docker Desktop's uninstaller takes `docker` with it.
Hopper uses those binaries internally for Compose compatibility; it does not
modify your machine-wide `PATH`. If you want the bundled CLI in a shell, add
the release's sidecar directory to your shell configuration: on macOS that is
`/Applications/Hopper.app/Contents/MacOS/sidecars`, while the Linux and
Windows archives contain `sidecars/` next to the Hopper executable. You can
also use the CLI installed separately by your package manager.
