// HTTP entry point for the Docker MCP — the bearer-authenticated, remote
// surface tandem reaches over the LAN. Launch it with:
//
//   HOPPER_MCP_TOKEN=… HOPPER_MCP_DOCKER_HOST=… bun src/mcp/serve.ts
//
// It points the Docker client at the isolated engine (HOPPER_MCP_DOCKER_HOST),
// then serves the same tool catalog as the stdio server over POST /mcp. With
// no token it refuses to start (fail closed).

import * as system from "../host/docker/system.ts";
import { applySandboxEngine } from "./engine.ts";
import { startHttp } from "./http.ts";
import { sandboxConfig } from "./sandbox.ts";

const cfg = sandboxConfig();
const where = applySandboxEngine(cfg);

const server = startHttp("hopper", "1.0.0");
if (!server) process.exit(1);

process.stderr.write(
  `[hopper-mcp] HTTP transport on http://${server.hostname}:${server.port}/mcp → ${where}\n`,
);
process.stderr.write(
  `[hopper-mcp] sandbox: network=${cfg.network} autoRemove=${cfg.autoRemove} pids=${cfg.pidsLimit} mem=${cfg.memoryBytes}B\n`,
);

if (!(await system.ping())) {
  process.stderr.write("[hopper-mcp] Docker engine unreachable — tools will report errors\n");
}
