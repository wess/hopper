// Standalone MCP server — exposes Hopper's Docker management to AI clients
// (Claude Desktop, Cursor, Claude Code, …) over stdio. Launch it with:
//
//   bun /absolute/path/to/hopper/src/mcp.ts
//
// then register that command in the client's MCP config. This process talks
// directly to the Docker Engine API via the host/docker modules (which import
// no app/butter code), so it runs independently of the desktop app.
//
// IMPORTANT: this process must never write to stdout except MCP protocol
// frames, so nothing here may `console.log` (stderr is fine for diagnostics).

import { createMcpServer } from "@basket/mcp";
import { z } from "zod";
import { req } from "./host/docker/client.ts";
import * as containers from "./host/docker/containers.ts";
import * as images from "./host/docker/images.ts";
import * as networks from "./host/docker/networks.ts";
import * as system from "./host/docker/system.ts";
import * as volumes from "./host/docker/volumes.ts";

// Strip Docker's stdcopy 8-byte frame headers from a raw log/exec buffer.
// Each frame: byte0=stream type, bytes1-3=0, bytes4-7=uint32 BE payload size,
// then the payload. If the bytes don't look framed (TTY mode), return as-is.
const demux = (buf: Uint8Array): string => {
  const decoder = new TextDecoder();
  let out = "";
  let i = 0;
  while (i + 8 <= buf.length) {
    const type = buf[i];
    const framed =
      (type === 0 || type === 1 || type === 2) &&
      buf[i + 1] === 0 &&
      buf[i + 2] === 0 &&
      buf[i + 3] === 0;
    if (!framed) return decoder.decode(buf.slice(i));
    const size = (buf[i + 4]! << 24) | (buf[i + 5]! << 16) | (buf[i + 6]! << 8) | buf[i + 7]!;
    const start = i + 8;
    const end = Math.min(start + size, buf.length);
    out += decoder.decode(buf.slice(start, end));
    i = end;
  }
  if (i < buf.length) out += decoder.decode(buf.slice(i));
  return out;
};

// Naive shell-ish split for the exec command string. Good enough for the
// "ls -la /" style inputs an AI client will send; compose users use the app.
const splitCmd = (cmd: string): string[] => cmd.trim().split(/\s+/).filter(Boolean);

const server = createMcpServer({
  name: "hopper",
  version: "1.0.0",
  description: "Manage Docker (containers, images, volumes, networks) through Hopper.",
});

server
  .tool<{ all?: boolean }>({
    name: "docker_list_containers",
    description: "List Docker containers. Set `all` to include stopped containers.",
    inputSchema: { all: z.boolean().optional().describe("Include stopped containers") },
    handler: async ({ all }) => {
      try {
        const list = await containers.list(all ?? false);
        return list.map((c) => ({
          id: c.id,
          name: c.name,
          image: c.image,
          state: c.state,
          status: c.status,
          ports: c.ports,
        }));
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string }>({
    name: "docker_inspect_container",
    description: "Inspect a container by id or name, returning the full Docker inspect JSON.",
    inputSchema: { id: z.string().describe("Container id or name") },
    handler: async ({ id }) => {
      try {
        return await containers.inspect(id);
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string; tail?: number }>({
    name: "docker_container_logs",
    description: "Fetch the most recent log lines from a container (one-shot, not following).",
    inputSchema: {
      id: z.string().describe("Container id or name"),
      tail: z.number().optional().describe("Number of trailing lines (default 200)"),
    },
    handler: async ({ id, tail }) => {
      try {
        const res = await req(`/containers/${id}/logs`, {
          query: { stdout: true, stderr: true, tail: tail ?? 200, follow: false },
        });
        const buf = new Uint8Array(await res.arrayBuffer());
        return { logs: demux(buf) };
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string }>({
    name: "docker_start_container",
    description: "Start a stopped container.",
    inputSchema: { id: z.string().describe("Container id or name") },
    handler: async ({ id }) => {
      try {
        await containers.start(id);
        return { ok: true };
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string }>({
    name: "docker_stop_container",
    description: "Stop a running container.",
    inputSchema: { id: z.string().describe("Container id or name") },
    handler: async ({ id }) => {
      try {
        await containers.stop(id);
        return { ok: true };
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string }>({
    name: "docker_restart_container",
    description: "Restart a container.",
    inputSchema: { id: z.string().describe("Container id or name") },
    handler: async ({ id }) => {
      try {
        await containers.restart(id);
        return { ok: true };
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string; force?: boolean }>({
    name: "docker_remove_container",
    description: "Remove a container. Set `force` to remove a running container.",
    inputSchema: {
      id: z.string().describe("Container id or name"),
      force: z.boolean().optional().describe("Force-remove a running container"),
    },
    handler: async ({ id, force }) => {
      try {
        await containers.remove(id, force);
        return { ok: true };
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ id: string; cmd: string }>({
    name: "docker_exec",
    description:
      "Run a command inside a running container and return its combined output (best-effort, capped).",
    inputSchema: {
      id: z.string().describe("Container id or name"),
      cmd: z.string().describe('Command to run, e.g. "ls -la /"'),
    },
    handler: async ({ id, cmd }) => {
      try {
        const created = await req(`/containers/${id}/exec`, {
          method: "POST",
          body: { AttachStdout: true, AttachStderr: true, Cmd: splitCmd(cmd) },
        });
        const { Id } = (await created.json()) as { Id: string };
        const started = await req(`/exec/${Id}/start`, {
          method: "POST",
          body: { Detach: false, Tty: false },
        });
        const buf = new Uint8Array(await started.arrayBuffer());
        const text = demux(buf);
        return { output: text.length > 10000 ? `${text.slice(0, 10000)}\n…(truncated)` : text };
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ all?: boolean }>({
    name: "docker_list_images",
    description: "List Docker images. Set `all` to include intermediate layers.",
    inputSchema: { all: z.boolean().optional().describe("Include intermediate images") },
    handler: async ({ all }) => {
      try {
        const list = await images.list(all ?? false);
        return list.map((i) => ({
          id: i.id,
          repoTags: i.repoTags,
          size: i.size,
          created: i.created,
        }));
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool<{ ref: string }>({
    name: "docker_pull_image",
    description: "Pull an image by reference (e.g. `nginx:latest`) to completion.",
    inputSchema: { ref: z.string().describe("Image reference, e.g. nginx:latest") },
    handler: async ({ ref }) => {
      try {
        return await images.pull("mcp", ref, () => {});
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool({
    name: "docker_list_volumes",
    description: "List Docker volumes.",
    handler: async () => {
      try {
        return await volumes.list();
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool({
    name: "docker_list_networks",
    description: "List Docker networks.",
    handler: async () => {
      try {
        return await networks.list();
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool({
    name: "docker_system_info",
    description: "Docker engine system info (counts, version, host details).",
    handler: async () => {
      try {
        return await system.info();
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  })
  .tool({
    name: "docker_system_df",
    description: "Docker disk usage summary (images, containers, volumes, build cache).",
    handler: async () => {
      try {
        return await system.df();
      } catch (e) {
        return { error: (e as Error).message };
      }
    },
  });

server
  .resource({
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
  })
  .resource({
    uri: "hopper://system",
    name: "Docker system",
    description: "Docker engine version and host info summary.",
    mimeType: "text/plain",
    handler: async () => {
      try {
        const [v, i] = await Promise.all([system.version(), system.info()]);
        return [
          `Engine: ${v.version} (API ${v.apiVersion})`,
          `Host: ${i.name} — ${i.operatingSystem} / ${i.architecture}`,
          `CPUs: ${i.ncpu}, Memory: ${i.memTotal} bytes`,
          `Containers: ${i.containers} (running ${i.containersRunning}, paused ${i.containersPaused}, stopped ${i.containersStopped})`,
          `Images: ${i.images}`,
        ].join("\n");
      } catch (e) {
        return `Error: ${(e as Error).message}`;
      }
    },
  });

if (!(await system.ping())) {
  process.stderr.write("[hopper-mcp] Docker engine unreachable — tools will report errors\n");
}

await server.serve();
