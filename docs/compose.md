# Compose — design notes

## Problem

A developer's containers almost never live alone — they come up as a stack
defined by a `docker-compose.yml`. Hopper needs to (a) make sense of stacks
that are already running and (b) bring stacks up and down from a file, with the
full set of compose options, the way Docker Desktop's Compose view does.

## Surfaces

Compose now has a **first-class Stacks view** (`src/app/components/stacks.tsx`),
alongside the inline stack grouping that still lives in the Containers view.

### Listing — label-driven, no CLI required

Every container the daemon returns carries its `com.docker.compose.project` /
`.service` / `.project.config_files` / `.project.working_dir` labels.
`compose.listProjects()` (`src/host/docker/compose/list.ts`) groups containers
by project and reconstructs a `ComposeProject` (services, aggregate status,
config-file paths) entirely from those labels. This means the Stacks view works
against **any** engine — even one with no compose CLI installed; you can still
see and stop existing stacks.

Aggregate status: **running** if every member runs, **stopped** if none do,
**partial** otherwise.

### Lifecycle + `up` from a file — CLI-backed

Reimplementing the compose spec over the Engine API (`depends_on`, healthcheck
gating, profiles, `extends`, interpolation, merge semantics) is a tarpit and a
permanent maintenance burden. Hopper instead **drives an external compose CLI**:

- `src/host/docker/compose/runner.ts` resolves a runner once, in preference
  order: a **bundled standalone `compose` binary** (so the feature works with no
  docker CLI installed) → the **`docker compose` v2 plugin** → the legacy
  hyphenated **`docker-compose` v1**. The args always start with `compose`;
  the standalone + v1 binaries take them with that subcommand stripped.
- `src/host/docker/compose/args.ts#composeArgs` is the pure, unit-tested
  argument builder. It supports the full option set: multiple `-f` files,
  `-p` project, `--env-file`, `--profile`, and for `up` — `--build`,
  `--force-recreate`, `--remove-orphans`; for `down` — `--volumes`, `--rmi`.
- `src/host/docker/compose/run.ts#runCompose` streams stdout+stderr line-by-line
  through `composeProgress` events keyed by a requestId (the same streaming
  pattern as pull/build).
- One unified `compose:action` channel covers `up` / `down` / `start` / `stop`
  / `restart` / `remove`. Lifecycle ops on an existing stack run label-driven
  (just `-p <project>`); only `up` needs the file(s).

### File viewer / editor

`src/host/docker/compose/files.ts` adds:

- `validateConfig(files, project)` — defers to `docker compose config`, which
  parses, merges, and normalizes the file set exactly as a real `up` would, so
  the editor catches errors *before* launching.
- `readComposeFile` / `writeComposeFile` — load and save a compose file on the
  host filesystem, backing the editor in the stack detail's **File** tab.

### Feature gating

`composeAvailable` (`runner.available()`) gates the CLI-backed actions. When no
runner is found, `up`/validate/edit are unavailable but the Stacks list and
label-driven stop still work — it fails closed, honestly surfaced in the UI.

## Trade-off

The CLI-backed half depends on a compose binary (bundled, v2, or v1). That's an
explicit choice: correctness and zero spec-drift in exchange for one external
dependency, surfaced honestly in the UI rather than papered over.
