// One-shot `docker.exec` for the MCP surface — run an argv vector inside an
// already-running container and return its combined output (capped).
//
// DESTRUCTIVE / high-privilege: exec runs arbitrary code inside a live
// container with that container's user and mounts, so it sidesteps the
// run/build sandbox hardening (those flags are set at create time and can't be
// applied to an existing container). The tool is flagged `destructive: true`
// so a downstream approval gate (tandem) can require confirmation before it
// fires. The command is an argv array — never split here.

import { json, req } from "../host/docker/client.ts";
import { demux } from "./demux.ts";

export type ExecResult = {
  readonly ok: boolean;
  readonly exitCode?: number;
  readonly output?: string;
  readonly error?: string;
};

const MAX_OUTPUT = 32 * 1024;

export const exec = async (container: string, cmd: string[]): Promise<ExecResult> => {
  if (!container.trim()) return { ok: false, error: "container is required" };
  if (!cmd || cmd.length === 0) return { ok: false, error: "cmd (argv array) is required" };
  try {
    const created = await json<{ Id: string }>(`/containers/${container}/exec`, {
      method: "POST",
      body: { AttachStdout: true, AttachStderr: true, Tty: false, Cmd: cmd },
    });
    const started = await req(`/exec/${created.Id}/start`, {
      method: "POST",
      body: { Detach: false, Tty: false },
    });
    let output = demux(new Uint8Array(await started.arrayBuffer()));
    if (output.length > MAX_OUTPUT) output = `${output.slice(0, MAX_OUTPUT)}\n…(truncated)`;

    // The exit code is only known after start; inspect the exec instance.
    const inspected = await json<{ ExitCode: number | null; Running: boolean }>(
      `/exec/${created.Id}/json`,
    );
    const exitCode = inspected.ExitCode ?? undefined;
    return { ok: exitCode === 0 || exitCode === undefined, exitCode, output };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};
