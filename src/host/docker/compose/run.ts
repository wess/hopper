// Run a compose lifecycle action, streaming stdout/stderr lines through
// `onProgress`, and resolve when the process exits.

import type {
  ComposeAction,
  ComposeOptions,
  ComposeProgress,
  ComposeTarget,
} from "../../../shared/types.ts";
import { composeArgs } from "./args.ts";
import { childEnv, resolveRunner, toCommand } from "./runner.ts";

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

export const runCompose = async (
  requestId: string,
  action: ComposeAction,
  target: ComposeTarget,
  options: ComposeOptions | undefined,
  onProgress: (p: ComposeProgress) => void,
): Promise<{ ok: boolean; error?: string }> => {
  const runner = await resolveRunner();
  if (!runner) {
    const error =
      "Docker Compose is not available (no bundled binary, `docker compose` plugin, or `docker-compose` found).";
    onProgress({ requestId, line: "", stream: "stderr", done: true, error });
    return { ok: false, error };
  }

  const proc = Bun.spawn(toCommand(runner, composeArgs(action, target, options)), {
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
  const error = ok ? undefined : `docker compose ${action} exited with code ${proc.exitCode}`;
  onProgress({ requestId, line: "", stream: "stdout", done: true, error });
  return { ok, error };
};
