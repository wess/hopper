---
title: Containers
group: Use
order: 1
summary: The list, the per-row actions, and the detail pane with logs, stats, files and a shell.
---

The Containers view is the one you will live in. A live list, a search box, an
**only running** filter, and per-row lifecycle buttons.

## The list

Each row shows the name, a state chip, the image, and published ports as
`host→container`. A container with a healthcheck gets a second chip — `healthy`
or `unhealthy` — because a container can be *running* and *unhealthy* at the
same time, and one green dot for both would be a lie.

A container that belongs to a Compose project gets a `docker` badge and groups
under [Stacks](stacks.html).

Search matches name, image and id, because those are the three things people
actually type.

## Actions

**Start**, **Stop** and **Restart** per row. Select several and act on all of
them at once — a failure on one is reported against that row and the rest
continue, so stopping twelve containers does not stop at the first one that was
already down.

On Apple Containers, restart is a stop followed by a start, because Apple has no
single restart command. A container that was already down still comes up.

## The detail pane

Click a row and a pane opens beside the list, so you never lose your place.

### Logs

Live, streaming, with stdout and stderr demuxed and stderr coloured. The buffer
is capped, so a chatty container cannot grow it without bound.

On Apple Containers the two streams arrive interleaved — the CLI does not demux
them the way the Engine API's framing does — so every line is reported as stdout
rather than guessed at.

### Stats

Live CPU, memory, network and block-IO meters. Engine API engines only.

### Files

Browse the container's filesystem, read a file, write one back, or export a path
as a tar. Engine API engines only.

### Terminal

An interactive shell, over a hijacked socket with a real TTY. Engine API engines
only.

### Inspect

The raw JSON the engine reports, for when a view has abstracted away the one
field you need.

> Stats, Files and Terminal do not appear on Apple Containers. They are not
> broken there — they are not implemented yet, and drawing three tabs that
> cannot load is worse than not drawing them. See [Engines](engines.html).
