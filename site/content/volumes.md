---
title: Volumes and networks
group: Use
order: 3
summary: Named volumes, bind mounts, and how Apple's networking differs.
---

## Volumes

Create, inspect, remove and prune. Sizes come from the engine's disk-usage report
where it has one.

Two things about volumes on **Apple Containers** are worth knowing before they
bite you:

- **A named volume is a real ext4 filesystem**, so it arrives with a
  `lost+found` directory in it. Postgres refuses to initialise into a non-empty
  directory, so point `PGDATA` at a subdirectory:

  ```yaml
  environment:
    PGDATA: /var/lib/postgresql/data/pgdata
  ```

- **Anonymous volumes are not removed with their container.** Docker cleans them
  up on `--rm`; Apple does not. Prune them from this view.

## Bind mounts

Apple's runtime bind-mounts host directories directly, so there is no share list
to maintain — which is a simplification over the VM Hopper used to run, where a
path outside the shared set silently showed the container an empty directory.

Two caveats:

- only `ro` is honoured as a mount option
- a bind mount performs worse than a named volume, so put databases on volumes

## Networks

Create, inspect, remove and prune. On macOS 26 Apple gives each container an
address on its network, reachable from the host and from other containers on the
same network, with DNS by container name. Separate networks are genuinely
isolated from one another.

Attaching a *running* container to a network is an Engine API feature. Apple
attaches containers to their networks when it creates them, so Hopper refuses
rather than pretending.
