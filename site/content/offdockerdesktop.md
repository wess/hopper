---
title: Move off Docker Desktop
group: Tutorials
order: 2
summary: Import into Hopper Engine, verify the replacement, then remove Docker Desktop safely.
---

Hopper Engine is a private Docker environment owned by Hopper. You can run it
beside Docker Desktop while you move, so the source stays intact until the new
workloads have been verified.

## 1. Take stock and back up data

Keep Docker Desktop running. In **Settings → Coexist or replace**, Hopper lists
the local runtimes it finds and explains the cost of running both.

Review `docker ps -a`, `docker images`, `docker volume ls`, and any databases or
named volumes you care about. Back those volumes up independently. Hopper's
import deliberately does not copy volume contents.

## 2. Start Hopper Engine

Choose Hopper Engine and start it. First startup downloads the pinned Linux base,
creates the private VM, starts Docker, and waits for the daemon to answer.

Starting Hopper Engine does not stop, change, or remove Docker Desktop. Both can
run at once, though they use extra memory and published ports may conflict.

## 3. Import

Open **Import**, select Docker Desktop as the source, and choose the images,
containers, networks, and volume definitions you need. Images stream directly
between the two Engine API endpoints. Containers are recreated from their
configuration; their writable layers are scratch and do not move.

The source is pinned for the duration of the run and nothing is deleted there.
One failed item is reported without abandoning everything else.

## 4. Restore volume data

Create each destination volume and restore its independent backup before
starting the container that owns it. One portable pattern is:

```sh
# Save from Docker Desktop.
docker --context desktop-linux run --rm \
  -v pgdata:/data -v "$PWD":/backup alpine \
  tar czf /backup/pgdata.tgz -C /data .

# Restore through Hopper Engine after installing its Docker CLI context.
docker --context hopper volume create pgdata
docker --context hopper run --rm \
  -v pgdata:/data -v "$PWD":/backup alpine \
  tar xzf /backup/pgdata.tgz -C /data
```

Use the real source context and volume names from your machine. Database-native
backup and restore tools are safer than filesystem copies for a live database.

## 5. Verify the replacement

Start the imported workloads on Hopper Engine. Verify ports, logs, healthchecks,
bind-mount paths, data, and any tools that mount the Docker socket. The Hopper
Docker context points normal `docker`, Compose, and Engine API tools at the
private socket.

## 6. Review removal

Only after independent backups and replacement verification, return to
**Settings → Coexist or replace**. Hopper shows the exact vendor uninstaller,
its destructive effects, and requires the application name as confirmation. It
opens the review in Terminal; it never removes another runtime merely because
you switched engines or completed an import.

After Docker Desktop is gone, use **Settings → Docker CLI → Install Docker CLI
tools** to repair command links and select the Hopper context if necessary.

## Limits to check before removing anything

- Volume contents do not move automatically.
- A service with only `build:` cannot be built by Hopper; provide an `image:` or
  build it elsewhere first.
- Bind mounts must refer to paths shared with Hopper Engine.
- Containers that depend on a specific desktop runtime or privileged host
  integration need workload-specific testing.
- Running both engines can conflict on published host ports.

The safe order is backup, import, restore, verify, and only then removal.
