---
title: Troubleshooting
group: Reference
order: 3
summary: The failures people actually hit, and what they mean.
---

## "Apple Containers can't run on this Mac"

It needs macOS 26. The vmnet APIs that give containers their own addresses do not
exist earlier. Use a Docker engine instead — Hopper still works as a client.

## The engine says "stopped" and Start does nothing

Check what Apple's own services think:

```sh
container system status
container system start
```

If `system start` complains about a kernel, let it install one:

```sh
container system start --enable-kernel-install
```

Hopper always passes that flag, because the command otherwise prompts on stdin
and Hopper has no terminal to answer with.

## Postgres exits immediately on Apple Containers

Its data directory is not empty. Apple's named volumes are real ext4
filesystems and arrive with a `lost+found`, which Postgres refuses to initialise
into. Point `PGDATA` at a subdirectory:

```yaml
environment:
  PGDATA: /var/lib/postgresql/data/pgdata
```

## Something bind-mounts /var/run/docker.sock and fails

Portainer, Traefik's Docker provider, and most CI-in-a-container setups do this.
Apple publishes no Docker socket, so there is nothing to mount. These need a
Docker engine.

## "no matching manifest for linux/arm64"

An x86-only image. Ask for it explicitly:

```sh
container run --platform linux/amd64 someimage
```

## The Stacks tab disappeared

You are on Apple Containers, which has no Compose. See [Stacks](stacks.html).

## Permission denied on the socket (Linux)

You are not in the `docker` group:

```sh
sudo usermod -aG docker "$USER"   # then log out and back in
```

For rootless Podman, the socket needs starting:

```sh
systemctl --user start podman.socket
```

## Hopper connects to the wrong daemon

`DOCKER_HOST` wins over everything when it is set. Unset it, or set
`HOPPER_ENGINE` to pick a provider explicitly:

```sh
HOPPER_ENGINE=apple /Applications/Hopper.app/Contents/MacOS/hopper
```

## Checking the app is really notarized

```sh
spctl -a -t exec -vv /Applications/Hopper.app
# accepted / source=Notarized Developer ID
```

Checking the `.dmg` instead will report *no usable signature* even on a perfectly
good release — the DMG is not codesigned, the ticket is stapled to it, and the
signature lives on the app inside.
