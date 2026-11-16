---
title: Troubleshooting
group: Reference
order: 3
summary: Diagnose Hopper Engine startup, socket selection, mounts, and optional backends.
---

## Hopper Engine does not become connected

The VM being up is only half the check: Docker inside it must answer Hopper's
private socket. Open **Settings → Engine** for the current phase and error, then
stop and start Hopper Engine once. First startup can take longer while the Linux
base is downloaded and provisioned.

If you changed CPU, memory, file shares, or Docker daemon JSON, restart the
engine to apply it. Invalid daemon JSON is rejected before it replaces the last
working configuration.

Hopper Engine data lives under `~/.hopper/engine/lima`. Do not delete that
directory as a troubleshooting step: it contains the VM disk and container data.

## A bind mount is empty or missing

Add its host path under **Settings → Hopper Engine → File sharing**, save, then
restart Hopper Engine. The private VM cannot expose paths outside its configured
shares.

## A published port is already in use

Another container or a second local runtime may own the host port. Stop the
conflicting workload or publish a different port. Docker Desktop and Hopper
Engine can coexist, so check both while migrating.

## Hopper connects to the wrong daemon

An explicit `HOPPER_ENGINE` overrides the saved choice, and `DOCKER_HOST` can
select an existing endpoint. Remove the override or pin the provider under
**Settings → Which engine**:

```sh
HOPPER_ENGINE=vm /Applications/Hopper.app/Contents/MacOS/hopper
```

Hopper keeps an unavailable explicit selection instead of falling through to an
unrelated daemon.

## Docker commands still target Docker Desktop

Use **Settings → Docker CLI → Install Docker CLI tools**. It repairs Hopper's
bundled command links and creates or updates the `hopper` context. Confirm it:

```sh
docker context use hopper
docker info
```

## Apple Containers cannot run on this Mac

The optional Apple backend needs Apple silicon and macOS 26. Use Hopper Engine
or another Docker-compatible endpoint instead.

If Apple's services are installed but stopped:

```sh
container system status
container system start --enable-kernel-install
```

Hopper passes `--enable-kernel-install` because the plain command can prompt on
stdin and the app has no terminal to answer.

## PostgreSQL exits on Apple Containers

Apple named volumes may contain `lost+found`. Point PostgreSQL at a subdirectory:

```yaml
environment:
  PGDATA: /var/lib/postgresql/data/pgdata
```

## Permission denied on a Linux socket

For Docker, add the user to the appropriate group and start a new login session:

```sh
sudo usermod -aG docker "$USER"
```

For rootless Podman, start its user socket:

```sh
systemctl --user start podman.socket
```

## Verify notarization

```sh
spctl -a -t exec -vv /Applications/Hopper.app
```

Check the app, not the DMG. The signature lives on `Hopper.app`; the notarization
ticket is stapled to the disk image.
