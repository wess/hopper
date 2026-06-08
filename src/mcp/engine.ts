// Point the Docker client at the isolated MCP engine.
//
// The docker client (host/docker/client.ts) reads its target from the active
// endpoint on every request. When the MCP is run as a sandboxed exec surface,
// we want it talking to a throwaway daemon — not whatever the developer's
// desktop app is using. If HOPPER_MCP_DOCKER_HOST is set we parse it and pin
// the active endpoint to it; otherwise we leave the ambient resolution alone
// (DOCKER_HOST → DOCKER_SOCKET → platform default).

import { parseDockerHost, setEndpoint } from "../host/docker/endpoint.ts";
import type { SandboxConfig } from "./sandbox.ts";

// Returns a human description of where the MCP will talk, for the startup log.
export const applySandboxEngine = (cfg: SandboxConfig): string => {
  if (!cfg.dockerHost) return "ambient engine (DOCKER_HOST / default socket)";
  const ep = parseDockerHost(cfg.dockerHost);
  if (!ep) {
    process.stderr.write(
      `[hopper-mcp] HOPPER_MCP_DOCKER_HOST is not a recognized docker host: ${cfg.dockerHost}\n`,
    );
    return "ambient engine (HOPPER_MCP_DOCKER_HOST unparseable — ignored)";
  }
  setEndpoint(ep);
  return cfg.dockerHost;
};
