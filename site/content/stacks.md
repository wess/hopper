---
title: Compose stacks
group: Use
order: 4
summary: Stacks reconstructed from container labels, and why the view hides itself on Apple Containers.
---

Hopper groups containers into projects using the `com.docker.compose.*` labels
the Compose CLI writes. That means stacks show up whether or not a Compose
binary is installed — the grouping is read from what is already running.

Each project lists its services with their states, and the whole project can be
started or stopped together.

## On Apple Containers

Apple ships no Compose, so **the Stacks view is not shown** when that engine is
active. It is not an empty list or a broken button; the route leaves the sidebar.

This is the biggest single gap between the two backends, and it is worth being
plain about: if your workflow is `docker compose up`, Apple Containers is not
there yet. Switch to a Docker engine under **Settings → Engine** and everything
works as before.

## What Hopper ships

The release bundles a standalone Compose binary and the Docker CLI, found at
runtime beside the executable. Docker Desktop's uninstaller takes `docker` with
it, so shipping one is what makes Hopper a replacement rather than a client that
depends on the thing it replaces.
