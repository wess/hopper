# Hopper

A native desktop app for managing Docker — a Docker Desktop–style client built
on the [Butter](https://github.com/wess/butter) framework. TypeScript on both
sides (Bun host + React 19 webview), no bundled browser, talks directly to the
Docker Engine API over the local unix socket.

## Features

- **Dashboard** — running/total containers, image/volume/network counts, disk
  usage with reclaimable meters, full engine/system info, and a live activity
  feed off the Docker event stream. One-click "Clean up" (system prune).
- **Containers** — list with **compose-stack grouping**, search, "only running"
  filter, multi-select bulk actions, and per-row lifecycle (start/stop/restart/
  pause/kill/remove/rename). Each compose stack header shows an aggregate
  up/total state badge and **stack-level start/stop/restart/remove**. Detail
  pane with tabs:
  - **Logs** — live streaming, demuxed stdout/stderr
  - **Stats** — live CPU / memory / network / block-IO meters
  - **Terminal** — interactive `/bin/sh` exec session (real TTY via socket hijack)
  - **Processes** — `top` of the container
  - **Inspect** — full JSON tree
  - **Run a container** dialog (image, ports, env, volumes, restart policy)
- **Compose** — bring stacks **up/down from a compose file** (file picker,
  optional project name, live streamed output). Backed by the `docker compose`
  CLI; the button only appears when the CLI is installed. See `docs/compose.md`.
- **Images** — list with in-use state, **pull with live layer progress**,
  **build from a Dockerfile** (context picker, build args, target, `.dockerignore`,
  live build log), **push to a registry** (credentials reused from your docker
  config), **Docker Hub search**, tag, history, inspect, remove, and prune.
  See `docs/build.md` and `docs/push.md`.
- **Volumes** — list with size/usage, create, inspect, remove, prune.
- **Networks** — list, create (driver/subnet/gateway/internal/attachable),
  inspect, connect/disconnect containers, remove, prune (built-ins protected).
- **Registry search** — one search across **Docker Hub**, **GitHub/GHCR**, and
  **Quay.io**, merged and ranked, each result one click from a pull.
- **Workspaces** — saved, named scopes (by compose project and/or name pattern)
  that filter the Containers and Images views; switch from the sidebar, manage
  in Settings. See `docs/workspaces.md`.
- **Menubar (status-bar) app** — a 🐳 item with live engine/running-container
  status, quick navigation, and quit, so Hopper runs like Docker Desktop.
- **MCP server** — a standalone Model Context Protocol server (`bun run mcp`)
  exposing Docker tools (list/start/stop/remove containers, logs, exec, images,
  pull, volumes, networks, system info/df) so AI clients can manage Docker.
  Copy the launch config from Settings → MCP Server.
- Command palette (⌘K), **light / dark / system** theme (⌘⇧L cycles), collapsible
  sidebar, native menu, auto-refresh off the engine event stream, and live
  connect/disconnect detection.

## Run

```bash
bun install
bun run dev        # opens the native window (Docker must be running)
```

Build a distributable:

```bash
bun run bundle     # .app bundle (macOS)
bun run package    # .dmg installer
```

Run just the MCP server (for AI clients):

```bash
bun run mcp        # stdio MCP server exposing Docker tools
```

Tests / checks:

```bash
bun test           # unit tests (scoped to src/ via bunfig.toml)
bunx tsc --noEmit  # typecheck
bunx biome check src
```

## Architecture

```
src/
  host/                       Bun backend (runs in the Butter host process)
    docker/                   Docker Engine API client + domain modules
      client.ts               unix-socket fetch wrapper + ndjson / log-demux streaming
      containers.ts images.ts volumes.ts networks.ts system.ts
      stats.ts logs.ts exec.ts
      build.ts                image build (classic builder) — tar context + /build stream
      compose.ts              compose up/down via the docker compose CLI
      credentials.ts          registry auth resolved from the user's docker config
    index.ts                  IPC handler wiring + stream/exec session management
    menu.ts                   native menu
  shared/
    types.ts                  the host <-> webview data contract
    channels.ts               typed IPC channels + events (@basket/ipc)
  app/                        React 19 webview
    components/               one folder per view (containers, images, …) + ui primitives
    lib/                      ipc facade + formatters
    state/                    tiny external store
    styles.css                design system (Docker-blue, light/dark)
```

The host reaches Docker over whichever transport `docker/endpoint.ts` resolves
from the environment — a unix socket (Linux/macOS), a Windows named pipe, or
TCP for remote daemons — honoring `DOCKER_HOST` (e.g. `tcp://host:2376`,
`npipe:////./pipe/docker_engine`), then `DOCKER_SOCKET`, then a per-platform
default. The API is pinned to `v1.43`. Streaming endpoints (logs, stats, events, pull)
arrive as host→webview events; interactive exec hijacks the socket for a raw
TTY. Built with Butter's vendored framework + the `basket` workspace packages
(`@basket/ipc`, `@basket/ui`, `@basket/window`, `@basket/menu`, `@basket/store`).
```
