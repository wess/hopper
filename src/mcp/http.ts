// HTTP transport for Hopper's Docker MCP.
//
// The stdio server (index.ts) stays for local operator use; this exposes the
// same tool catalog over a bearer-authenticated POST /mcp so a remote client
// (tandem on the LAN) can drive the sandboxed exec surface. Mirrors Tangle's
// HTTP-wrap pattern: unauthenticated GET /mcp for discovery, authenticated
// POST /mcp for JSON-RPC.
//
// Env:
//   HOPPER_MCP_HTTP_HOST   Bind address (default 127.0.0.1 — loopback-only).
//                          Set 0.0.0.0 to reach it across the LAN.
//   HOPPER_MCP_HTTP_PORT   Bind port (default 8420).
//   HOPPER_MCP_TOKEN       Bearer token (REQUIRED — see auth.ts).
//   plus the sandbox / engine env documented in sandbox.ts.

import { isAuthorized, readToken } from "./auth.ts";
import { type Dispatcher, handleRpc, type JsonRpcRequest } from "./dispatch.ts";
import { dockerResources, dockerTools } from "./tools.ts";

export type HttpMcpOptions = {
  readonly name: string;
  readonly version: string;
  readonly host?: string;
  readonly port?: number;
  readonly token: string;
};

const json = (status: number, body: unknown): Response =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });

const dispatcher = (name: string, version: string): Dispatcher => ({
  serverInfo: { name, version },
  tools: dockerTools(),
  resources: dockerResources(),
});

type McpServerHandle = ReturnType<typeof Bun.serve>;

export const serveHttp = (opts: HttpMcpOptions): McpServerHandle => {
  const host = opts.host ?? process.env.HOPPER_MCP_HTTP_HOST ?? "127.0.0.1";
  const port = opts.port ?? (Number(process.env.HOPPER_MCP_HTTP_PORT) || 8420);
  const d = dispatcher(opts.name, opts.version);

  return Bun.serve({
    hostname: host,
    port,
    fetch: async (request) => {
      const url = new URL(request.url);
      if (url.pathname !== "/mcp") return json(404, { error: "Not found" });

      // Unauthenticated discovery — clients confirm MCP is on and learn the
      // bearer requirement without holding a token yet.
      if (request.method === "GET") {
        return json(200, {
          server: { name: opts.name, version: opts.version },
          endpoint: "/mcp",
          auth: "Bearer (HOPPER_MCP_TOKEN) on POST /mcp",
          tools: d.tools.map((t) => ({ name: t.name, destructive: t.destructive === true })),
        });
      }

      if (request.method !== "POST") return json(405, { error: "Method not allowed" });

      if (!isAuthorized(request.headers.get("authorization"), opts.token)) {
        return new Response(JSON.stringify({ error: "Unauthorized" }), {
          status: 401,
          headers: {
            "Content-Type": "application/json",
            "WWW-Authenticate": "Bearer",
          },
        });
      }

      let body: JsonRpcRequest;
      try {
        body = (await request.json()) as JsonRpcRequest;
      } catch {
        return json(400, {
          jsonrpc: "2.0",
          id: null,
          error: { code: -32700, message: "Parse error" },
        });
      }

      const res = await handleRpc(d, body);
      // Notifications return null — ack with 202 and no body.
      if (res === null) return new Response(null, { status: 202 });
      return json(200, res);
    },
  });
};

// Start the HTTP transport, failing closed if no token is configured.
export const startHttp = (name: string, version: string): McpServerHandle | null => {
  const token = readToken();
  if (!token) {
    process.stderr.write(
      "[hopper-mcp] HOPPER_MCP_TOKEN is not set — refusing to start the HTTP transport (it would be unauthenticated).\n",
    );
    return null;
  }
  return serveHttp({ name, version, token });
};
