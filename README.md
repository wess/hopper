# Hopper

A native desktop app that both **manages** Docker and **is** a Docker engine —
a Docker Desktop replacement written in Rust. The UI is [gpui](https://github.com/zed-industries/zed)
+ [guise](https://github.com/wess/guise); the async layer is Tokio; the whole
thing talks to the Docker Engine API directly and, on macOS, runs its own Linux
VM (Apple Virtualization.framework) with `dockerd` inside it, so you can
uninstall Docker Desktop entirely.

No bundled browser, no Electron, no sidecar processes — the VM is created
in-process, so the app itself carries `com.apple.security.virtualization`.

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

### The managed engine (macOS)

On macOS Hopper can supply its own engine rather than attaching to one:

- Boots a minimal Linux guest (raw arm64 kernel + Alpine initramfs) on
  Virtualization.framework, entirely from Rust.
- Runs `dockerd` inside it and bridges its socket out to
  `~/.hopper/run/docker.sock`.
- **Forwards published container ports to `localhost`** — `docker run -p 8080:80`
  is reachable at `http://localhost:8080` on the Mac.
- **Shares host directories into the guest** — a bind mount outside `$HOME`
  reaches the container instead of resolving to an empty directory.

If no managed engine is available (or you prefer one you already run), Hopper
falls back to any existing engine — Docker Desktop, Colima, Rancher Desktop, a
remote daemon over TCP.

## Architecture

A Cargo workspace, layered bottom-up. Each crate depends only on those below it,
and the gpui-free core never imports gpui.

```
crates/
  model     shared domain types (the wire contract)
  store     ~/.hopper/ JSON persistence + OS keychain
  docker    Engine API client (hyper over unix/tcp/npipe) + every domain module
  engine    provider abstraction; the macOS VM (objc2 + Virtualization.framework)
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

An engine must be reachable, or (on macOS, with the guest image built) Hopper
starts its own.

## Build a release

```sh
scripts/build/guest.sh      # build the guest kernel + initramfs (Linux/buildx)
scripts/bundle.sh           # assemble + sign dist/Hopper.app
scripts/dmg.sh              # package dist/Hopper.dmg
```

Pushing a version bump in `Cargo.toml` to `main` tags, builds, notarizes, and
publishes via `.github/workflows/release.yml`.

## Sponsor

♥ [Sponsor this project](https://github.com/sponsors/wess)
