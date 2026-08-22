# Hopper

A native desktop app for running and managing containers — a Docker Desktop
replacement written in Rust. The UI is [gpui](https://github.com/zed-industries/zed)
+ [guise](https://github.com/wess/guise); the async layer is Tokio.

On **macOS** the engine is [Apple's `container`](https://github.com/apple/container)
(macOS 26+): every container is its own lightweight VM, maintained by Apple, and
Hopper installs and drives it for you. On **Linux** Hopper uses whichever of
Docker or Podman you have. Either way you can uninstall Docker Desktop — and
**Import from Docker** brings your images and containers across first.

No bundled browser, no Electron, no sidecar processes, and no entitlements
beyond files and networking.

## Features

- **Dashboard** — running/total containers, image/volume/network counts, disk
  usage with reclaimable meters, and one-click "Clean up" (system prune).
- **Containers** — live list with search, "only running" filter, and per-row
  lifecycle (start/stop/restart). A detail pane opens beside the list with tabs:
  - **Logs** — live streaming, demuxed stdout/stderr, stderr coloured
  - **Stats** — live CPU / memory / network / block-IO / PID meters
  - **Files** — browse the container's filesystem and copy files out
  - **Terminal** — a real interactive shell (socket hijack + a minimal terminal
    emulator, not a line-buffered fake)
  - **Inspect** — the full JSON tree
- **Images** — list with in-use state, pull with live layer progress, build from
  a Dockerfile (`.dockerignore` honoured, including negations), push, tag,
  history, remove, prune, save/load.
- **Volumes** — list with in-use detection, create, remove, prune.
- **Networks** — list, create, connect/disconnect, remove, prune (built-ins
  protected).
- **Stacks** — compose projects reconstructed from container labels, so they
  appear with no compose CLI and start/stop label-driven.
- **Settings** — engine control, VM resources, file-sharing paths, Docker CLI
  integration.
- **Migration** — scan a source engine (Docker Desktop / Colima / Rancher) and
  copy images, volumes, and networks into Hopper's engine.
- **MCP server** — a standalone stdio Model Context Protocol server
  (`hoppermcp`) exposing Docker tools to AI clients.

### Engines

- **macOS — Apple Containers.** Needs macOS 26. If it is not installed, Hopper
  offers to fetch Apple's signed installer and hands it to the system installer;
  Hopper never elevates. After that it starts and stops the services itself.
- **Linux — Docker or Podman.** Whichever is there, rootless sockets checked
  before the system ones, so a desktop Podman is not passed over for a stale
  root daemon. Podman's socket is Docker-compatible, so nothing else changes.
- **Anywhere — an engine you already run.** Docker Desktop, Colima, Rancher
  Desktop, or a remote daemon over TCP.

Apple's runtime is not the Engine API, and Hopper does not pretend otherwise:
pause, rename, post-create resource changes, restart policies, healthchecks and
the event stream do not exist there, so the UI hides them rather than offering a
button that fails. Anything that needs them can switch engines in Settings.

### Import from Docker

Copies images, networks and containers out of Docker Desktop, Colima or Rancher
Desktop and into whichever engine Hopper is running. Containers are recreated
rather than moved — the image, ports, mounts and labels travel; a writable layer
is by definition scratch. Nothing is removed from the source.

## Architecture

A Cargo workspace, layered bottom-up. Each crate depends only on those below it,
and the gpui-free core never imports gpui.

```
crates/
  model     shared domain types (the wire contract)
  store     ~/.hopper/ JSON persistence + OS keychain
  docker    Engine API client (hyper over unix/tcp/npipe) + every domain module
  apple     Apple Containers, driven through the `container` CLI
  engine    provider abstraction; Apple / Docker-or-Podman / existing
  migrate   Docker Desktop → Hopper migration
  host      the async service facade the UI calls
  mcp       the stdio MCP server (hoppermcp)
  app       the gpui + guise application (hopperdev / hopper)
```

The async Docker layer runs on a Tokio runtime; gpui has its own executor. The
two meet at one seam — `app::bridge` — which runs a future on Tokio and
delivers the result on the gpui main thread. Streaming calls (logs, stats,
events, exec) are cancelled by dropping the producer, so a closed view closes
its stream with no separate abort registry. This mirrors the
[tables](https://github.com/wess/tables) architecture.

## Run

```sh
cargo run -p app            # launch the app (dev binary: hopperdev)
cargo test                  # the whole suite
cargo clippy --all-targets  # lint
cargo run -p mcp            # the stdio MCP server
```

An engine must be reachable. On macOS that means Apple's `container` — Hopper's
first-run panel installs it if it is missing.

## Build a release

```sh
scripts/bundle.sh           # assemble + sign dist/Hopper.app
scripts/dmg.sh              # package dist/Hopper.dmg
```

Pushing a version bump in `Cargo.toml` to `main` tags, builds, notarizes, and
publishes via `.github/workflows/release.yml`.

## Sponsor

♥ [Sponsor this project](https://github.com/sponsors/wess)
