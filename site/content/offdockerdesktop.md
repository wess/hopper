---
title: Move off Docker Desktop
group: Tutorials
order: 2
summary: Bring your images and containers across, then uninstall the thing you were paying for.
---

You have Docker Desktop with real work in it. This moves that work onto Apple's
runtime and gets Docker Desktop off the machine.

Read the whole thing before starting — there is one step that needs manual work.

## 1. Take stock

With Docker Desktop still running, open Hopper. It will connect to it — the
footer says `Connected to Docker …`.

Note what you actually need. `docker ps -a`, `docker images`, `docker volume ls`.
Most people find half of it is scratch.

## 2. Install Apple Containers

**Settings → Engine**, or the first-run panel. Install it, but **do not switch
yet** — you want Docker running as the import source.

## 3. Import

Go to **Import** and click **Look for Docker**.

Hopper finds Docker Desktop's socket, lists everything it holds, and preselects
all of it. Untick what you do not want — this is the moment to leave the scratch
behind.

Click **Import**. Images stream out of Docker, land on disk as tars, and load
into Apple's runtime. Containers are recreated from their configuration.

**Nothing is removed from Docker.** If this goes wrong, you have lost time, not
work.

## 4. Deal with volumes by hand

Hopper will tell you, per volume:

> Volume contents are not copied yet.

This is the manual step. For each volume with data you care about, the shape is:
tar it out of Docker, create the volume on the new engine, tar it back in.

```sh
# out of Docker
docker run --rm -v pgdata:/data -v "$PWD":/backup alpine \
  tar czf /backup/pgdata.tgz -C /data .

# into Apple Containers
container volume create pgdata
container run --rm --volume pgdata:/data --volume "$PWD":/backup alpine \
  tar xzf /backup/pgdata.tgz -C /data
```

If that volume is a Postgres data directory, read the
[volumes page](volumes.html) first — Apple's named volumes contain a
`lost+found`, and Postgres refuses to initialise into a non-empty directory.

## 5. Switch

**Settings → Engine → Apple Containers**. The footer reconnects, and your
imported containers are in the list.

Check the things that matter: do they start, do their ports answer, does the data
look right.

## 6. Then, and only then

```sh
/Applications/Docker.app/Contents/MacOS/uninstall
```

Docker Desktop's uninstaller takes the `docker` CLI with it. Hopper bundles its
own — **Settings → Docker CLI** puts it on your `PATH`.

---

## Before you commit

Some things do not survive the move, and it is better to know now:

- **Compose stacks.** Apple ships no Compose. If `docker compose up` is your
  daily driver, stay on a Docker engine — see [Stacks](stacks.html).
- **Anything that bind-mounts `/var/run/docker.sock`** — Portainer, Traefik's
  Docker provider, most CI-in-a-container setups. Apple publishes no such socket.
- **Containers needing `NET_ADMIN` or host kernel modules**, WireGuard being the
  usual example. Each container is its own VM, so there is no host kernel to
  reach into.
- **x86-only images** need an explicit `--platform linux/amd64`.

None of these are Hopper limitations; they are the shape of Apple's runtime.
Hopper's job is to tell you about them before you find out the hard way.
