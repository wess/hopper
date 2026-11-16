---
title: Docker CLI
group: Use
order: 6
summary: Point docker and docker compose at the engine Hopper is running.
---

Hopper is a GUI, not a replacement for your terminal. Both should talk to the
same engine.

## With Hopper Engine

Use **Settings → Docker CLI → Install Docker CLI tools**. Hopper installs links
to its bundled Docker, Compose, Buildx, and credential helper for the current
user, then creates or updates a `hopper` context pointing at the private socket:

```sh
docker context use hopper
docker info
```

The setup also repairs links that targeted a removed Docker Desktop bundle.

## With another Engine API engine

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

The release ships Docker, Compose, Buildx, and the macOS credential helper in
`Hopper.app`, because Docker Desktop's uninstaller can remove the commands that
used to point into its bundle. Hopper's setup action creates user-owned links;
it does not replace unrelated working binaries.
