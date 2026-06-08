// The tool shape shared by both MCP transports (stdio + HTTP).
//
// It extends @basket/mcp's ToolDef idea with two things the sandboxed exec
// surface needs:
//
//   • a zod `shape` for the input — the SDK's registerTool takes a zod raw
//     shape directly, and the HTTP transport converts the same shape to JSON
//     Schema for tools/list, so there is one source of truth.
//   • a `destructive` flag — surfaced in tools/list so a downstream approval
//     gate (tandem) can require confirmation before calling it.

import type { z } from "zod";

export type ZodShape = Record<string, z.ZodType>;

export type McpTool = {
  readonly name: string;
  readonly description: string;
  readonly shape: ZodShape;
  // True for tools that run arbitrary code / mutate the engine in ways an
  // operator should confirm (docker.exec, docker.run, docker.build, compose).
  readonly destructive?: boolean;
  readonly handler: (input: Record<string, unknown>) => unknown | Promise<unknown>;
};

export type McpResource = {
  readonly uri: string;
  readonly name: string;
  readonly description: string;
  readonly mimeType: string;
  readonly handler: () => string | Promise<string>;
};
