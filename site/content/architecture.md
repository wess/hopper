---
title: Architecture
group: Reference
order: 2
summary: A bottom-up Rust workspace with a gpui-free core and one Tokio-to-gpui seam.
---

```
crates/
  model     shared domain types and wire contracts
  store     ~/.hopper JSON persistence + OS keychain
  docker    Engine API client, transports, streaming, exec, archives
  apple     optional Apple Containers CLI adapter
  engine    provider registry + Hopper's private Lima VM
  compose   Compose parsing, interpolation, planning and naming
  migrate   source discovery and Docker-to-Hopper import
  host      async service facade and backend dispatch
  mcp       stdio MCP server
  app       gpui + guise desktop application
  site      documentation generator
```

Each crate depends only on layers below it. The core stays free of gpui; the UI
boundary is `app`.

## Providers and backends

On macOS, Hopper Engine provisions a private Lima VM under
`~/.hopper/engine/lima`, starts rootful Docker inside it, publishes a private
host socket, and waits for Docker readiness. The bundled helper owns Apple's
Virtualization framework integration, file sharing, and port forwarding.

Docker, Podman, Hopper Engine, and remote daemons all converge on `crates/docker`.
Endpoints are resolved per request and API versions negotiate down for older
daemons. Unix sockets, TCP/TLS, and Windows named pipes use the same client.

Apple Containers is the second backend. `host::runtime::Backend` is an enum,
not a trait, because streaming operations accept `FnMut` callbacks that are not
object-safe. `EngineCapabilities` tells the UI which controls each backend can
actually support.

## Why Apple uses a CLI adapter

Apple's `container` talks to its apiserver over XPC and does not publish a
Docker Engine API. `crates/apple` therefore drives the binary and maps its JSON
onto Hopper's shared model. The decoder is deliberately liberal because Apple
only promises output stability within a patch version.

## The async seam

Docker work runs on Tokio while gpui owns the application thread. `app::bridge`
is the single handoff: it runs a future on Tokio and delivers its result to gpui
through a runtime-agnostic channel. Streaming views cancel by dropping their
receivers, so logs, stats, and events do not need a separate abort registry.

This layering follows the [tables](https://github.com/wess/tables) architecture.
