// A minimal MCP JSON-RPC dispatcher over the shared tool/resource catalog.
//
// The stdio entry point uses @basket/mcp's createMcpServer (SDK + stdio
// transport). The HTTP transport can't reuse that — @basket/mcp only serves
// over stdio and exposes no request handler — so this implements the handful
// of MCP methods an AI client calls over a plain POST /mcp, mirroring the
// pattern Tangle uses (src/mcp/http.ts in the tangle repo). Tool input shapes
// are advertised as JSON Schema derived from the same zod shapes the stdio
// server registers, so the two transports stay in lockstep.

import { z } from "zod";
import type { McpResource, McpTool } from "./types.ts";

const PROTOCOL_VERSION = "2024-11-05";

export type JsonRpcRequest = {
  readonly jsonrpc?: string;
  readonly id?: string | number | null;
  readonly method?: string;
  readonly params?: Record<string, unknown>;
};

export type JsonRpcResponse = {
  readonly jsonrpc: "2.0";
  readonly id: string | number | null;
  readonly result?: unknown;
  readonly error?: { code: number; message: string };
};

const ok = (id: string | number | null, result: unknown): JsonRpcResponse => ({
  jsonrpc: "2.0",
  id,
  result,
});

const err = (id: string | number | null, code: number, message: string): JsonRpcResponse => ({
  jsonrpc: "2.0",
  id,
  error: { code, message },
});

const stringify = (value: unknown): string => {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

// tools/list advertises name, description, JSON-Schema inputSchema, and a
// `destructive` annotation so an approval gate can act on it.
const toolListEntry = (t: McpTool) => ({
  name: t.name,
  description: t.description,
  inputSchema: z.toJSONSchema(z.object(t.shape)),
  annotations: { destructiveHint: t.destructive === true },
});

export type Dispatcher = {
  readonly serverInfo: { name: string; version: string };
  readonly tools: readonly McpTool[];
  readonly resources: readonly McpResource[];
};

export const handleRpc = async (
  d: Dispatcher,
  body: JsonRpcRequest,
): Promise<JsonRpcResponse | null> => {
  const id = body.id ?? null;
  if (body.jsonrpc !== "2.0" || typeof body.method !== "string") {
    return err(id, -32600, "Invalid JSON-RPC request");
  }

  switch (body.method) {
    case "initialize":
      return ok(id, {
        protocolVersion: PROTOCOL_VERSION,
        capabilities: { tools: {}, resources: {} },
        serverInfo: d.serverInfo,
      });
    // Notifications carry no id and expect no response body.
    case "notifications/initialized":
    case "notifications/cancelled":
      return null;
    case "ping":
      return ok(id, {});
    case "tools/list":
      return ok(id, { tools: d.tools.map(toolListEntry) });
    case "tools/call": {
      const name = body.params?.name as string | undefined;
      const args = (body.params?.arguments as Record<string, unknown>) ?? {};
      const tool = d.tools.find((t) => t.name === name);
      if (!tool) return err(id, -32602, `Unknown tool: ${name}`);
      try {
        const parsed = z.object(tool.shape).parse(args);
        const result = await tool.handler(parsed as Record<string, unknown>);
        return ok(id, { content: [{ type: "text", text: stringify(result) }] });
      } catch (e) {
        return ok(id, {
          isError: true,
          content: [{ type: "text", text: e instanceof Error ? e.message : String(e) }],
        });
      }
    }
    case "resources/list":
      return ok(id, {
        resources: d.resources.map((r) => ({
          uri: r.uri,
          name: r.name,
          description: r.description,
          mimeType: r.mimeType,
        })),
      });
    case "resources/read": {
      const uri = body.params?.uri as string | undefined;
      const resource = d.resources.find((r) => r.uri === uri);
      if (!resource) return err(id, -32602, `Unknown resource: ${uri}`);
      const text = await resource.handler();
      return ok(id, {
        contents: [{ uri: resource.uri, mimeType: resource.mimeType, text }],
      });
    }
    default:
      return err(id, -32601, `Method not found: ${body.method}`);
  }
};
