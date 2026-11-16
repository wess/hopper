---
title: Volumes and networks
group: Use
order: 3
summary: Persistent data, host bind mounts, and container networking across Hopper's engines.
---

## Volumes

Create, inspect, remove, and prune named volumes. Hopper Engine and other Engine
API providers report usage through Docker's disk-usage endpoint. A volume
outlives the container that mounts it; deleting a container does not delete its
data.

Imports recreate volume definitions but do not copy their contents. Back up and
restore important data independently before starting a migrated workload.

On the optional Apple Containers backend, a named volume is an ext4 filesystem
and may contain `lost+found`. Point PostgreSQL at a subdirectory when needed:

```yaml
environment:
  PGDATA: /var/lib/postgresql/data/pgdata
```

## Bind mounts

Hopper Engine shares configured host paths with its private VM. Add a path under
**Settings → Hopper Engine → File sharing**, restart the engine, then mount it
into a container. A path outside the shared set cannot expose its files.

Prefer named volumes for database storage and bind mounts for source trees,
configuration, and files that need direct host editing.

Apple Containers bind-mounts host directories directly and accepts a smaller
set of mount options. The UI reports unsupported options instead of dropping
them silently.

## Networks

Create, inspect, remove, and prune Docker networks. Hopper Engine supports the
normal Engine API operations, including attaching or detaching a running
container. Published ports are forwarded back to the Mac.

Apple Containers attaches networks at container creation and cannot connect a
running container afterward. Hopper hides or refuses that unsupported action.
