---
title: Import from Docker
group: Use
order: 5
summary: Copy images, networks and containers out of Docker Desktop into whichever engine Hopper runs.
---

Hopper's pitch on macOS is that you stop running Docker Desktop, and that is only
true if what you already have comes with you.

**Import** scans for another engine, lists what it holds, and copies your
selection across. **Nothing is removed from the source.** If the import goes
wrong, Docker is exactly where it was.

## What it finds

Docker Desktop, Colima and Rancher Desktop, in that order, plus the classic
system socket:

- `~/.docker/run/docker.sock`
- `~/.colima/default/docker.sock`
- `~/.rd/docker.sock`
- `/var/run/docker.sock`

Whichever engine Hopper is currently running is excluded, so it never offers to
copy an engine onto itself.

## How it copies

**Images.** Into an Engine API engine, a tar streams straight from `/images/get`
to `/images/load`. Into Apple's runtime there is no socket to load into, so the
tar lands on disk first and `container image load` reads it back — slower, and
the only way in.

**Containers** are recreated, not moved. The image, ports, mounts and labels
travel; the writable layer does not, because it is by definition scratch. That is
what makes a stack come back up.

**Networks** are recreated on Engine API engines. On Apple they are not, because
Apple attaches a container to its networks when it creates it.

## What it does not copy yet

**Volume contents.** The volume is listed and you can select it, and Hopper will
tell you plainly that the data did not move:

> Volume contents are not copied yet. Create the volume on this engine and move
> the data yourself before starting the container that needs it.

That warning matters more than it looks. A user who assumes their database volume
came across finds out when the container starts empty, which is exactly the
failure worth being loud about.

## Safety

The source engine is **pinned when you scan**, so a daemon coming up or down
mid-import cannot quietly redirect it somewhere else. Per-item failures are
reported against that item and the run continues — one unreadable image should
not cost you the other nineteen.

Bind mounts are called out too: a container that mounts a host path gets a
warning, because that path has to exist on the destination for the mount to mean
anything.
