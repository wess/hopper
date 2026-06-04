# Compose — design notes

## Problem

A developer's containers almost never live alone — they come up as a stack
defined by a `docker-compose.yml`. Hopper needs to (a) make sense of stacks
that are already running and (b) bring stacks up and down from a file, the way
Docker Desktop's Compose view does.

## Two phases, deliberately separate

Compose is split because the two halves have very different cost and risk.

### Phase 1 — stack grouping + lifecycle (pure client)

Every container the daemon returns already carries its
`com.docker.compose.project` / `.service` labels, surfaced on `Container` as
`composeProject` / `composeService`. The Containers view groups by project (it
already did), and each stack header now shows an aggregate state badge and
Start / Stop / Restart / Remove buttons.

- Aggregate state comes from `stackState()` (`src/app/lib/compose.ts`):
  **running** if every member runs, **stopped** if none do, **partial**
  otherwise.
- Stack actions reuse the existing `containers:batch` channel — the header
  buttons just pass the member ids. No new host code, no daemon features.

This covers the common case (managing stacks that are already up) with almost
no new surface.

### Phase 2 — `compose up` / `down` from a file (CLI-backed)

Reimplementing the compose spec over the Engine API (`depends_on`, healthcheck
gating, profiles, `extends`, interpolation, merge semantics) is a tarpit and a
permanent maintenance burden. Hopper instead **drives the official
`docker compose` CLI**:

- `src/host/docker/compose.ts` shells `docker compose -f <file> [-p <name>] up -d
  --remove-orphans` / `down`, streaming stdout+stderr line-by-line through
  `composeProgress` events keyed by a requestId (the same streaming pattern as
  pull/build).
- `composeArgs()` is the pure, unit-tested argument builder.
- The feature is **gated on `composeAvailable`** (`docker compose version`
  succeeds). When the CLI isn't installed, the Compose button is hidden — Hopper
  stays a pure socket client everywhere else, and this is the one spot that
  isn't, so it fails closed.

## Trade-off

Phase 2 depends on the `docker compose` CLI being on `PATH`. That's an explicit
choice: correctness and zero spec-drift in exchange for one external dependency,
surfaced honestly in the UI rather than papered over.
