---
title: Architecture
group: Reference
order: 2
summary: A Rust workspace layered bottom-up, with one Tokio-to-gpui seam.
---

```
crates/
  model     shared domain types (the wire contract)
  store     ~/.hopper/ JSON persistence + OS keychain
  docker    Engine API client (hyper) + every domain module
  apple     Apple Containers, driven through the `container` CLI
  engine    provider abstraction; Apple / Docker-or-Podman / existing
  migrate   Docker -> Hopper import
  host      the async service facade the UI calls
  mcp       the stdio MCP server
  app       the gpui + guise application
  site      this documentation site's generator
```

Each crate depends only on those below it, and the gpui-free core never imports
gpui. The boundary is `app`.

## Two engines, one enum

Hopper speaks to two kinds of engine: the Docker Engine API over a socket, and
Apple's runtime over its CLI. `host::runtime::Backend` picks between them.

It is an **enum, not a trait**, and that is deliberate. The streaming calls take
closures — `FnMut(LogLine) -> bool` — which are not object-safe. Boxing every
callback to pretend a trait works would buy nothing over a two-variant match.

`EngineCapabilities` carries what each backend can actually do, and the UI reads
it: unavailable tabs are not drawn, unavailable routes leave the sidebar.

## The async seam

The Docker layer runs on a Tokio runtime; gpui has its own executor. They meet in
exactly one place, `app::bridge`, which runs a future on Tokio and delivers the
result on the gpui main thread through a runtime-agnostic oneshot.

Streaming works the same way: a producer sends items down a channel, and the view
that stops receiving is how a log follow gets cancelled. There is no separate
abort registry to keep in sync.

This mirrors the [tables](https://github.com/wess/tables) architecture.

## Why the CLI, not the API

Apple's `container` talks to `container-apiserver` over XPC and publishes no
Docker Engine API. [The request to expose one was closed as not
planned](https://github.com/apple/container/issues/636). So `crates/apple` drives
the binary and maps its JSON onto the same `model` types the Engine API path
produces — which is why no view knows which backend answered.

That mapping is deliberately liberal: Apple promises stability only within a
patch version, so every field is optional and anything structural Hopper does not
need stays a `Value`. A field Apple renames should cost one column in a list, not
the whole view.
