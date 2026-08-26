# Hopper

Repository guidance for agent sessions.

## What this is

Hopper is a native desktop app for running and managing containers — a Docker
Desktop replacement written entirely in Rust. The UI is
[gpui](https://github.com/zed-industries/zed) + [guise](https://github.com/wess/guise);
the async layer is Tokio.

It speaks to two kinds of engine. On **macOS** it drives
[Apple's `container`](https://github.com/apple/container) runtime, which needs
macOS 26 — each container is its own lightweight VM, maintained by Apple. On
**Linux** it attaches to whichever of Docker or Podman is installed. Anything
that answers the Docker Engine API (Docker Desktop, Colima, Rancher Desktop, a
remote daemon over TCP) works everywhere as a fallback.

The architecture follows [tables](https://github.com/wess/tables) — a Cargo
workspace layered bottom-up, gpui-free core crates, one Tokio↔gpui bridge.

> Ported from an earlier TypeScript/Bun + React build, and from a hand-rolled
> Virtualization.framework VM that Apple's runtime now supersedes on macOS.
> Nothing in the repo depends on Bun, Node, `butter`, `basket`, or a guest
> kernel any more.

## Commands

Rust throughout. No Bun, no npm.

```sh
cargo run -p app                 # launch the app (dev binary: hopperdev)
cargo build --workspace          # build everything
cargo test                       # the whole suite
cargo test -p docker             # one crate
cargo clippy --all-targets       # lint
cargo run -p mcp                 # the stdio MCP server (hoppermcp)
```

Release (macOS `.app` + `.dmg`):

```sh
scripts/bundle.sh        # assemble + sign dist/Hopper.app (CODESIGN_IDENTITY)
scripts/dmg.sh           # package dist/Hopper.dmg
```

## Architecture

A workspace layered bottom-up; each crate depends only on those below it, and
the gpui-free core never imports gpui.

- **`model`** — shared domain types, the wire contract every crate speaks. Pure
  serde; field names stay `camelCase` so `~/.hopper/` files and MCP JSON
  round-trip. One focused module per domain.
- **`store`** — local persistence: JSON documents under `~/.hopper/` (atomic
  writes, corrupt-file backup) and the OS keychain for secrets (single-line
  JSON per key — the macOS keychain corrupts values containing a newline).
  `HOPPER_DIR` overrides the root.
- **`docker`** — the Engine API client and every domain module. `client.rs` is
  hyper-over-transport (`transport.rs` covers unix/tcp/tls/npipe); it resolves
  the endpoint fresh per request (a provider can repoint it at runtime) and
  negotiates the API version down for older daemons. Streaming calls are
  cancelled by dropping the future. `demux.rs` is the stdcopy framing;
  `exec.rs` hijacks the socket for an interactive TTY (needs the
  `Upgrade: tcp` / `Connection: Upgrade` headers or the daemon won't 101);
  `archive.rs` copies files in/out and browses container filesystems.
- **`apple`** — Apple Containers, the macOS engine. `container` talks to its
  apiserver over XPC and publishes no Docker Engine API (the request to expose
  one was closed as not planned), so `cli.rs` drives the binary and `wire.rs`
  maps its JSON onto the same `model` types the Engine API path produces. Be
  liberal in what you accept there: Apple promises stability only within a
  patch version.
- **`engine`** — the provider abstraction (attach to an engine, or supply one).
  `providers/apple.rs` is the macOS engine, `providers/linux.rs` finds Docker or
  Podman (rootless sockets first), `providers/existing.rs` is the
  always-available fallback.
- **`migrate`** — Docker Desktop → Hopper migration (scan + copy).
- **`host`** — the async service facade (`Host`) the UI calls. Owns the Docker
  client, the engine registry, settings, and workspace scoping. gpui-free.
- **`mcp`** — the stdio MCP server; protocol framing plus the Docker tool set.
- **`app`** — the gpui + guise application. `bridge.rs` is the Tokio↔gpui seam;
  `state.rs` the cross-view signal contract; `views/` one module per surface.

### The two backends

`host::runtime::Backend` is an enum, not a trait: the streaming calls take
closures, and `FnMut(LogLine) -> bool` is not object-safe. `EngineCapabilities`
carries what each backend can actually do, so the UI hides what is missing
rather than offering a button that always fails. Apple's runtime has no pause,
no rename, no post-create resource update, no event stream and no healthchecks.

Two traps worth knowing:

- `container system start` reads from stdin when it is not told whether to
  install the default kernel. Hopper runs it with no terminal, so it always
  passes `--enable-kernel-install`.
- Apple renders JSON dates as ISO8601 (`Output.renderJSON` defaults to the
  `.compact` options), not Swift's reference-epoch default.

## Conventions

- Functional style; avoid classes/OO where a free function + plain data will do.
- Keep the gpui-free core (`model`, `store`, `docker`, `engine`, `migrate`,
  `host`, `mcp`) free of gpui — the boundary is `app`.
- File names lowercase, no spaces/dashes/underscores; split by directory, not
  compound names. Small, focused files.
- Pure logic carries the unit coverage (parsers, arg builders, framing, diffing,
  reducers). Integration seams — the `container` CLI, socket discovery, the
  import path — are where the real bugs hide; verify those against a live
  engine, not just tests.
- The workspace version in the root `Cargo.toml` drives releases.
