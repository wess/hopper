// Standalone MCP server (stdio) — exposes Hopper's Docker tools to AI clients
// (Claude Desktop, Cursor, Claude Code, …). Launch it with:
//
//   bun /absolute/path/to/hopper/src/mcp/index.ts
//
// then register that command in the client's MCP config. This process talks
// directly to the Docker Engine API via the host/docker modules (which import
// no app/butter code), so it runs independently of the desktop app.
//
// For a remote, bearer-authenticated surface (e.g. tandem over the LAN) use the
// HTTP transport instead: `bun src/mcp/serve.ts`.
//
// IMPORTANT: this process must never write to stdout except MCP protocol
// frames, so nothing here may `console.log` (stderr is fine for diagnostics).

import { createMcpServer } from "@basket/mcp";
import * as system from "../host/docker/system.ts";
import { applySandboxEngine } from "./engine.ts";
import { sandboxConfig } from "./sandbox.ts";
import { dockerResources, dockerTools } from "./tools.ts";

const cfg = sandboxConfig();
const where = applySandboxEngine(cfg);
process.stderr.write(`[hopper-mcp] stdio transport → ${where}\n`);

const server = createMcpServer({
  name: "hopper",
  version: "1.0.0",
  description: "Manage Docker (build, run, compose, logs, exec) through Hopper.",
});

for (const t of dockerTools()) {
  server.tool({
    name: t.name,
    description: t.description,
    inputSchema: t.shape,
    handler: (input) => t.handler((input ?? {}) as Record<string, unknown>),
  });
}

for (const r of dockerResources()) {
  server.resource({
    uri: r.uri,
    name: r.name,
    description: r.description,
    mimeType: r.mimeType,
    handler: r.handler,
  });
}

if (!(await system.ping())) {
  process.stderr.write("[hopper-mcp] Docker engine unreachable — tools will report errors\n");
}

await server.serve();
