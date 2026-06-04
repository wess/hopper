import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";

export type JsonSchema = Record<string, unknown>;

export type ToolDef<I, O> = {
  readonly name: string;
  readonly description?: string;
  readonly inputSchema?: JsonSchema;
  readonly handler: (input: I) => O | Promise<O>;
};

export type ResourceDef = {
  readonly uri: string;
  readonly name?: string;
  readonly description?: string;
  readonly mimeType?: string;
  readonly handler: () => string | Promise<string>;
};

export type McpServerOptions = {
  readonly name: string;
  readonly version: string;
  readonly description?: string;
};

export type BasketMcpServer = {
  readonly tool: <I = unknown, O = unknown>(def: ToolDef<I, O>) => BasketMcpServer;
  readonly resource: (def: ResourceDef) => BasketMcpServer;
  readonly serve: () => Promise<void>;
};

const stringifyResult = (value: unknown): string => {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

export const createMcpServer = (options: McpServerOptions): BasketMcpServer => {
  const server = new McpServer({ name: options.name, version: options.version });
  const tools: ToolDef<unknown, unknown>[] = [];
  const resources: ResourceDef[] = [];

  const api: BasketMcpServer = {
    tool: <I, O>(def: ToolDef<I, O>) => {
      tools.push(def as ToolDef<unknown, unknown>);
      return api;
    },
    resource: (def) => {
      resources.push(def);
      return api;
    },
    serve: async () => {
      for (const t of tools) {
        server.registerTool(
          t.name,
          {
            description: t.description,
            ...(t.inputSchema ? { inputSchema: t.inputSchema as never } : {}),
          },
          (async (input: unknown) => {
            const result = await t.handler(input);
            return {
              content: [{ type: "text", text: stringifyResult(result) }],
            };
          }) as never,
        );
      }

      for (const r of resources) {
        server.registerResource(
          r.name ?? r.uri,
          r.uri,
          {
            description: r.description,
            mimeType: r.mimeType ?? "text/plain",
          },
          async () => ({
            contents: [
              {
                uri: r.uri,
                mimeType: r.mimeType ?? "text/plain",
                text: await r.handler(),
              },
            ],
          }),
        );
      }

      const transport = new StdioServerTransport();
      await server.connect(transport);
    },
  };

  return api;
};
