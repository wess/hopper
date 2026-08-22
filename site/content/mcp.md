---
title: MCP server
group: Reference
order: 1
summary: Expose Docker tools to an AI client over stdio.
---

Hopper ships a standalone Model Context Protocol server that gives an AI client
tools for containers, images, volumes, networks and logs.

```sh
hoppermcp        # or: cargo run -p mcp
```

It speaks MCP over stdio: no socket, no port, no daemon of its own. It reaches
the engine the same way the app does, through the same host facade, so it obeys
the same engine selection and the same capability rules.

## Wiring it into Claude Code

```json
{
  "mcpServers": {
    "hopper": {
      "command": "/Applications/Hopper.app/Contents/MacOS/sidecars/hoppermcp"
    }
  }
}
```

## What it exposes

List and inspect containers, images, volumes and networks; start, stop and
restart containers; read logs; and report engine status.

Operations the active engine cannot perform return a clear refusal naming the
engine and the operation, rather than a generic failure — the same rule the UI
follows.
