// The Docker tool catalog for the MCP surface. One source of truth, consumed by
// both the stdio entry (index.ts) and the HTTP transport (http.ts).
//
// Tools that run code or mutate the engine (`docker.run`, `docker.build`,
// `docker.compose.up`, `docker.exec`) are marked `destructive`. `docker.exec`
// is the sharpest: it runs an argv vector inside a live container, bypassing
// the create-time sandbox hardening, so a downstream approval gate should
// always confirm it.

import { z } from "zod";
import { buildImage } from "../host/docker/build.ts";
import { req } from "../host/docker/client.ts";
import * as compose from "../host/docker/compose.ts";
import * as containers from "../host/docker/containers.ts";
import * as images from "../host/docker/images.ts";
import * as networks from "../host/docker/networks.ts";
import * as system from "../host/docker/system.ts";
import * as volumes from "../host/docker/volumes.ts";
import { demux } from "./demux.ts";
import { exec } from "./exec.ts";
import { run } from "./run.ts";
import { sandboxConfig } from "./sandbox.ts";
import type { McpResource, McpTool } from "./types.ts";

const fail = (e: unknown): { error: string } => ({
  error: e instanceof Error ? e.message : String(e),
});

// docker.compose.up writes a project file to a temp path before invoking the
// compose binary, since runCompose takes a -f file path. The caller passes
// the compose YAML inline OR a path already on the (sandbox) host.
const composeUp = async (input: { file: string; project?: string }) => {
  const lines: string[] = [];
  const res = await compose.runCompose("mcp", "up", input.file, input.project, (p) => {
    if (p.line) lines.push(`[${p.stream}] ${p.line}`);
  });
  return { ok: res.ok, error: res.error, output: lines.join("\n") };
};

export const dockerTools = (): McpTool[] => {
  const cfg = sandboxConfig();

  return [
    {
      name: "docker.build",
      description:
        "Build an image from a context directory (classic builder). Returns the built image id.",
      destructive: true,
      shape: {
        context: z.string().describe("Build context directory on the engine host"),
        dockerfile: z.string().optional().describe("Dockerfile path relative to context"),
        tag: z.string().optional().describe("name:tag to apply to the built image"),
      },
      handler: async (i) => {
        const input = i as { context: string; dockerfile?: string; tag?: string };
        const lines: string[] = [];
        const res = await buildImage(
          "mcp",
          { contextDir: input.context, dockerfile: input.dockerfile, tag: input.tag },
          (p) => {
            if (p.stream) lines.push(p.stream.trimEnd());
            else if (p.status) lines.push(p.status);
          },
        );
        return { ok: res.ok, error: res.error, imageId: res.imageId, log: lines.join("\n") };
      },
    },
    {
      name: "docker.run",
      description:
        'Run a command in a fresh, sandboxed container (auto-removed, no host mounts, dropped capabilities, constrained network) and return its output. `cmd` is an argv array, e.g. ["sh","-c","echo hi"].',
      destructive: true,
      shape: {
        image: z.string().describe("Image reference, e.g. alpine:latest"),
        cmd: z.array(z.string()).optional().describe("Command argv array (not a shell string)"),
        env: z.array(z.string()).optional().describe("Environment as KEY=VALUE strings"),
      },
      handler: (i) => {
        const input = i as { image: string; cmd?: string[]; env?: string[] };
        return run(cfg, input.image, input.cmd, input.env);
      },
    },
    {
      name: "docker.compose.up",
      description:
        "Bring a compose stack up (detached) from a compose file path on the engine host.",
      destructive: true,
      shape: {
        file: z.string().describe("Path to a docker-compose.yml on the engine host"),
        project: z.string().optional().describe("Compose project name"),
      },
      handler: (i) => composeUp(i as { file: string; project?: string }),
    },
    {
      name: "docker.logs",
      description: "Fetch the most recent log lines from a container (one-shot, not following).",
      shape: {
        container: z.string().describe("Container id or name"),
        tail: z.number().optional().describe("Number of trailing lines (default 200)"),
      },
      handler: async (i) => {
        const input = i as { container: string; tail?: number };
        try {
          const res = await req(`/containers/${input.container}/logs`, {
            query: { stdout: true, stderr: true, tail: input.tail ?? 200, follow: false },
          });
          return { logs: demux(new Uint8Array(await res.arrayBuffer())) };
        } catch (e) {
          return fail(e);
        }
      },
    },
    {
      name: "docker.exec",
      description:
        "Run an argv command inside a RUNNING container and return its combined output. DESTRUCTIVE: runs arbitrary code in a live container, bypassing the run sandbox — gate behind operator approval.",
      destructive: true,
      shape: {
        container: z.string().describe("Container id or name"),
        cmd: z.array(z.string()).describe('Command argv array, e.g. ["ls","-la","/"]'),
      },
      handler: (i) => {
        const input = i as { container: string; cmd: string[] };
        return exec(input.container, input.cmd);
      },
    },
    {
      name: "docker.list_containers",
      description: "List containers. Set `all` to include stopped ones.",
      shape: { all: z.boolean().optional().describe("Include stopped containers") },
      handler: async (i) => {
        try {
          const list = await containers.list((i as { all?: boolean }).all ?? false);
          return list.map((c) => ({
            id: c.id,
            name: c.name,
            image: c.image,
            state: c.state,
            status: c.status,
            ports: c.ports,
          }));
        } catch (e) {
          return fail(e);
        }
      },
    },
    {
      name: "docker.list_images",
      description: "List images. Set `all` to include intermediate layers.",
      shape: { all: z.boolean().optional().describe("Include intermediate images") },
      handler: async (i) => {
        try {
          const list = await images.list((i as { all?: boolean }).all ?? false);
          return list.map((m) => ({ id: m.id, repoTags: m.repoTags, size: m.size }));
        } catch (e) {
          return fail(e);
        }
      },
    },
    {
      name: "docker.list_volumes",
      description: "List volumes.",
      shape: {},
      handler: async () => {
        try {
          return await volumes.list();
        } catch (e) {
          return fail(e);
        }
      },
    },
    {
      name: "docker.list_networks",
      description: "List networks.",
      shape: {},
      handler: async () => {
        try {
          return await networks.list();
        } catch (e) {
          return fail(e);
        }
      },
    },
    {
      name: "docker.system_info",
      description: "Docker engine system info (counts, version, host details).",
      shape: {},
      handler: async () => {
        try {
          return await system.info();
        } catch (e) {
          return fail(e);
        }
      },
    },
  ];
};

export const dockerResources = (): McpResource[] => [
  {
    uri: "hopper://containers",
    name: "Running containers",
    description: "A human-readable summary of currently running containers.",
    mimeType: "text/plain",
    handler: async () => {
      try {
        const list = await containers.list(false);
        if (list.length === 0) return "No running containers.";
        return list.map((c) => `${c.name}  (${c.image})  — ${c.status}`).join("\n");
      } catch (e) {
        return `Error: ${(e as Error).message}`;
      }
    },
  },
];
