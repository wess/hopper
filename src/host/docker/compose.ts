// Compose orchestration. Hopper deliberately does not reimplement the compose
// spec (depends_on, healthchecks, profiles, merge semantics …); it drives the
// official Compose v2 binary and streams its output. It prefers a bundled
// standalone `compose` binary (so the feature works on a machine with no
// docker CLI installed — the point of being a self-contained engine) and falls
// back to the system `docker compose` plugin. Either way it targets the active
// engine via DOCKER_HOST.

import type { ComposeProgress } from "../../shared/types.ts";
import { resolveSidecar } from "../engine/sidecar.ts";
import { currentEndpoint, dockerHostValue } from "./endpoint.ts";

export type ComposeOp = "up" | "down";

// Build the `docker compose` argument vector (without the leading "docker").
// `up` runs detached and reaps orphaned containers; `down` tears the stack
// down. A project name is passed through when supplied.
export const composeArgs = (op: ComposeOp, file: string, project?: string): string[] => {
  const args = ["compose", "-f", file];
  if (project?.trim()) args.push("-p", project.trim());
  if (op === "up") args.push("up", "-d", "--remove-orphans");
  else args.push("down");
  return args;
};

// Resolve the command vector to run. A bundled standalone compose v2 binary
// takes the same args minus the leading "compose" subcommand; otherwise we
// invoke the `docker compose` plugin.
const composeCommand = (op: ComposeOp, file: string, project?: string): string[] => {
  const args = composeArgs(op, file, project);
  const bundled = resolveSidecar("compose");
  return bundled ? [bundled, ...args.slice(1)] : ["docker", ...args];
};

// Child env: point compose at whatever engine the client is currently using.
const childEnv = (): Record<string, string | undefined> => ({
  ...process.env,
  DOCKER_HOST: dockerHostValue(currentEndpoint()),
});

// Whether Compose can run: a bundled binary always counts; otherwise probe the
// system `docker compose` plugin.
export const available = async (): Promise<boolean> => {
  if (resolveSidecar("compose")) return true;
  try {
    const proc = Bun.spawn(["docker", "compose", "version"], {
      stdout: "ignore",
      stderr: "ignore",
      env: childEnv(),
    });
    await proc.exited;
    return proc.exitCode === 0;
  } catch {
    return false;
  }
};

// Pump a byte stream line-by-line into `onLine`, flushing any trailing partial
// line when the stream ends.
const pumpLines = async (
  stream: ReadableStream<Uint8Array>,
  onLine: (line: string) => void,
): Promise<void> => {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      let nl = buf.indexOf("\n");
      while (nl !== -1) {
        onLine(buf.slice(0, nl));
        buf = buf.slice(nl + 1);
        nl = buf.indexOf("\n");
      }
    }
  } finally {
    reader.releaseLock();
  }
  if (buf.length > 0) onLine(buf);
};

// Run `docker compose up|down`, streaming stdout/stderr lines through
// `onProgress`, and resolve when the process exits.
export const runCompose = async (
  requestId: string,
  op: ComposeOp,
  file: string,
  project: string | undefined,
  onProgress: (p: ComposeProgress) => void,
): Promise<{ ok: boolean; error?: string }> => {
  const proc = Bun.spawn(composeCommand(op, file, project), {
    stdout: "pipe",
    stderr: "pipe",
    env: childEnv(),
  });
  await Promise.all([
    pumpLines(proc.stdout, (line) =>
      onProgress({ requestId, line, stream: "stdout", done: false }),
    ),
    pumpLines(proc.stderr, (line) =>
      onProgress({ requestId, line, stream: "stderr", done: false }),
    ),
  ]);
  await proc.exited;
  const ok = proc.exitCode === 0;
  const error = ok ? undefined : `docker compose ${op} exited with code ${proc.exitCode}`;
  onProgress({ requestId, line: "", stream: "stdout", done: true, error });
  return { ok, error };
};
