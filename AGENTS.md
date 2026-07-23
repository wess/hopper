# AGENTS.md

Guidance for AI agents working in this repository (mirrors CLAUDE.md).

## What this is

Hopper is a native desktop app that both **manages** Docker and **is** a Docker
engine — a Docker Desktop replacement written entirely in Rust. The UI is
[gpui](https://github.com/zed-industries/zed) + [guise](https://github.com/wess/guise);
the async layer is Tokio; it talks to the Docker Engine API directly and, on
macOS, runs its own Linux VM via Apple's Virtualization.framework with `dockerd`
inside it.

The architecture follows [tables](https://github.com/wess/tables) — a Cargo
workspace layered bottom-up, gpui-free core crates, one Tokio↔gpui bridge.

> This was ported from an earlier TypeScript/Bun + React build. That
> implementation has been removed; nothing in the repo depends on Bun, Node,
> `butter`, or `basket` any more.

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
scripts/build/guest.sh   # guest kernel + initramfs (Linux/buildx; raw arm64 Image)
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
- **`engine`** — the provider abstraction (attach to an engine, or supply one).
  `providers/existing.rs` is the always-available fallback. `vz/` is the macOS
  managed engine: `vm.rs` builds the `VZVirtualMachineConfiguration` (objc2),
  `machine.rs` owns the lifecycle over a dispatch queue, `bridge.rs` serves the
  guest socket at `~/.hopper/run/docker.sock`, `forwarder.rs` forwards published
  ports to `localhost`, `shares.rs` maps host directories into the guest.
- **`migrate`** — Docker Desktop → Hopper migration (scan + copy).
- **`host`** — the async service facade (`Host`) the UI calls. Owns the Docker
  client, the engine registry, settings, and workspace scoping. gpui-free.
- **`mcp`** — the stdio MCP server; protocol framing plus the Docker tool set.
- **`app`** — the gpui + guise application. `bridge.rs` is the Tokio↔gpui seam;
  `state.rs` the cross-view signal contract; `views/` one module per surface.

### The macOS managed engine (`crates/engine/src/vz/`)

Replaces the Swift `hopperd` sidecar the Bun build shipped. Because a Rust app
has no JIT, `com.apple.security.virtualization` sits on the app itself and the
VM runs in-process — no sidecar, no inside-out signing. The guest
(`native/guest/`) is an Alpine + static Docker Engine initramfs plus a persistent
`/var/lib/docker` disk; the kernel + initrd are **data** and live in
`Contents/Resources/` (codesign rejects unsigned code under `MacOS/`).

## Conventions

- Functional style; avoid classes/OO where a free function + plain data will do.
- Keep the gpui-free core (`model`, `store`, `docker`, `engine`, `migrate`,
  `host`, `mcp`) free of gpui — the boundary is `app`.
- File names lowercase, no spaces/dashes/underscores; split by directory, not
  compound names. Small, focused files.
- Pure logic carries the unit coverage (parsers, arg builders, framing, diffing,
  reducers). Integration seams — the VM, vsock, port forwarding, virtiofs — are
  where the real bugs hide; verify those against a live engine, not just tests.
- The workspace version in the root `Cargo.toml` drives releases.
