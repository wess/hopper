// Sandboxed `docker.run` for the MCP exec surface.
//
// Unlike host/docker/containers.ts#run (which backs the desktop "Run" dialog
// and honors host bind-mounts, port publishing, restart policies, …), this
// creates a container with the hardened HostConfig from sandbox.ts and NO host
// exposure. The command is passed as an argv array — never split here — so a
// caller can run `["sh", "-c", "echo hi && ls"]` without us mis-tokenizing it.
//
// It runs to completion (no detach), then collects the container's combined
// output via the logs endpoint and returns it. With AutoRemove on, the
// container is gone by the time we read logs, so we wait on it first.

import { json, req } from "../host/docker/client.ts";
import { demux } from "./demux.ts";
import { hardenedHostConfig, type SandboxConfig } from "./sandbox.ts";

export type RunResult = {
  readonly ok: boolean;
  readonly exitCode?: number;
  readonly output?: string;
  readonly error?: string;
};

const MAX_OUTPUT = 64 * 1024;

// Build the create body. The hardened HostConfig is spread LAST so nothing in
// the caller's request can relax it.
const createBody = (
  cfg: SandboxConfig,
  image: string,
  cmd: string[] | undefined,
  env: string[] | undefined,
): Record<string, unknown> => ({
  Image: image,
  Cmd: cmd && cmd.length > 0 ? cmd : undefined,
  Env: env ?? [],
  // Detach stdio; we read output from the logs endpoint after the run.
  AttachStdin: false,
  AttachStdout: false,
  AttachStderr: false,
  Tty: false,
  HostConfig: hardenedHostConfig(cfg),
});

export const run = async (
  cfg: SandboxConfig,
  image: string,
  cmd?: string[],
  env?: string[],
): Promise<RunResult> => {
  if (!image.trim()) return { ok: false, error: "image is required" };
  let id: string | undefined;
  try {
    const created = await json<{ Id: string }>("/containers/create", {
      method: "POST",
      body: createBody(cfg, image, cmd, env),
    });
    id = created.Id;
    await req(`/containers/${id}/start`, { method: "POST" });

    // Wait for the container to exit so we can capture its code and logs before
    // AutoRemove tears it down.
    const waited = await json<{ StatusCode: number }>(`/containers/${id}/wait`, {
      method: "POST",
    });

    let output = "";
    try {
      const res = await req(`/containers/${id}/logs`, {
        query: { stdout: true, stderr: true, follow: false, tail: 2000 },
      });
      output = demux(new Uint8Array(await res.arrayBuffer()));
    } catch {
      // AutoRemove may have already reaped the container; output is best-effort.
    }
    if (output.length > MAX_OUTPUT) output = `${output.slice(0, MAX_OUTPUT)}\n…(truncated)`;

    return { ok: waited.StatusCode === 0, exitCode: waited.StatusCode, output };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};
