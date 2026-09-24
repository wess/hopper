# Hopper

A native desktop app for running and managing containers — a Docker Desktop
replacement written in Rust. The UI is [gpui](https://github.com/zed-industries/zed)
+ [guise](https://github.com/wess/guise); the async layer is Tokio.

Hopper currently targets **Apple silicon Macs running macOS 26 or later**. Its
engine is [Apple's `container`](https://github.com/apple/container): every
container is its own lightweight VM, maintained by Apple. Hopper installs and
drives the runtime for you. It can also attach to a Docker-compatible endpoint
on the Mac, and **Import from Docker** brings your images and containers across.

No bundled browser or Electron, no always-running sidecar or VM, and no
entitlements beyond files and networking. Release builds may include the
standalone Docker CLI and Compose binaries used on demand for compatibility.

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
- **Settings** — choose Automatic, Apple Containers, Docker, Podman, Colima,
  Rancher Desktop, or an existing endpoint; control lifecycle, appearance,
  per-container resource guidance, and Docker CLI integration.
- **Migration** — scan a source engine (Docker Desktop / Colima / Rancher) and
  copy images, networks, and container configuration into Hopper's engine;
  named volumes referenced by recreated containers are created as needed,
  while selected volume contents are called out for manual transfer rather
  than being silently lost.
- **MCP server** — a standalone stdio Model Context Protocol server
  (`hoppermcp`) exposing Docker tools to AI clients.

### Engines

- **macOS — Apple Containers.** Needs an Apple silicon Mac running macOS 26. If it is not installed, Hopper
  offers to fetch Apple's signed installer and hands it to the system installer;
  Hopper never elevates. After that it starts and stops the services itself.
- **Existing engines.** On macOS, Hopper can attach to Docker Desktop, Podman,
  Colima, Rancher Desktop, or a remote Docker-compatible endpoint.

Apple's runtime is not the Engine API, and Hopper does not pretend otherwise:
pause, rename, post-create resource changes, restart policies, healthchecks,
and the event stream do not exist there, so the UI hides
them or explains the limitation rather than offering a button that fails.
Anything that needs them can switch engines in Settings.

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

An engine must be reachable. That can be Apple's `container`, Docker,
Podman, Colima, Rancher Desktop, or another Docker-compatible endpoint;
Hopper's first-run panel offers Apple's signed installer when that runtime is
missing.

For an explicit endpoint, set `DOCKER_HOST`; Hopper also understands Podman's
standard `CONTAINER_HOST` when `DOCKER_HOST` is not set. A saved engine choice
in Settings takes precedence over automatic discovery, while these environment
variables are useful for remote daemons and shell-driven workflows.

## Build a release

```sh
scripts/bundle.sh           # assemble + sign dist/Hopper.app
scripts/dmg.sh              # package dist/Hopper.dmg
```

CI builds and tests on Apple silicon macOS. Releases include a notarized macOS
DMG for Apple silicon; the scripts above handle signing and notarization with
the configured `CODESIGN_IDENTITY` credentials.

## Sponsor

♥ [Sponsor this project](https://github.com/sponsors/wess)
