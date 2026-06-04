import { existsSync } from "node:fs";
import { join } from "node:path";

export type SpawnOptions = {
  readonly args?: readonly string[];
  readonly cwd?: string;
  readonly env?: Readonly<Record<string, string>>;
};

export type Sidecar = {
  readonly name: string;
  readonly pid: number;
  readonly write: (text: string) => void;
  readonly kill: (signal?: NodeJS.Signals | number) => void;
  readonly onStdout: (handler: (chunk: string) => void) => () => void;
  readonly onStderr: (handler: (chunk: string) => void) => () => void;
  readonly exited: Promise<number | null>;
};

const encoder = new TextEncoder();

const parseSidecarsEnv = (): Map<string, string> => {
  const map = new Map<string, string>();
  const raw = process.env.BUTTER_SIDECARS ?? "";
  if (!raw) return map;
  for (const entry of raw.split(":::")) {
    if (!entry) continue;
    const eq = entry.indexOf("==");
    if (eq <= 0) continue;
    map.set(entry.slice(0, eq), entry.slice(eq + 2));
  }
  return map;
};

const resolveSidecar = (name: string): string | null => {
  const direct = parseSidecarsEnv().get(name);
  if (direct && existsSync(direct)) return direct;
  const dir = process.env.BUTTER_SIDECARS_DIR;
  if (dir) {
    const ext = process.platform === "win32" ? ".exe" : "";
    const candidate = join(dir, `${name}${ext}`);
    if (existsSync(candidate)) return candidate;
  }
  return null;
};

export const listSidecars = (): readonly string[] => [...parseSidecarsEnv().keys()];

const pump = async (
  stream: ReadableStream<Uint8Array> | null | undefined,
  handlers: ((chunk: string) => void)[],
): Promise<void> => {
  if (!stream) return;
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  for (;;) {
    const { value, done } = await reader.read();
    if (done) break;
    if (!value) continue;
    const text = decoder.decode(value, { stream: true });
    for (const h of handlers) {
      try {
        h(text);
      } catch {
        // ignore handler errors
      }
    }
  }
};

export const spawn = (name: string, options: SpawnOptions = {}): Sidecar => {
  const path = resolveSidecar(name);
  if (!path) {
    throw new Error(
      `sidecar: "${name}" not found. Declare it in butter.yaml#bundle.sidecars or set BUTTER_SIDECARS_DIR.`,
    );
  }
  const proc = Bun.spawn([path, ...(options.args ?? [])], {
    cwd: options.cwd,
    env: { ...process.env, ...(options.env ?? {}) },
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });

  const stdoutHandlers: ((c: string) => void)[] = [];
  const stderrHandlers: ((c: string) => void)[] = [];
  void pump(proc.stdout as ReadableStream<Uint8Array>, stdoutHandlers);
  void pump(proc.stderr as ReadableStream<Uint8Array>, stderrHandlers);

  return {
    name,
    pid: proc.pid,
    write: (text) => {
      const stdin = proc.stdin as { write?: (chunk: Uint8Array) => unknown } | null | undefined;
      if (!stdin?.write) throw new Error("sidecar: stdin not writable");
      stdin.write(encoder.encode(text));
    },
    kill: (signal) => {
      proc.kill(signal ?? "SIGTERM");
    },
    onStdout: (h) => {
      stdoutHandlers.push(h);
      return () => {
        const i = stdoutHandlers.indexOf(h);
        if (i >= 0) stdoutHandlers.splice(i, 1);
      };
    },
    onStderr: (h) => {
      stderrHandlers.push(h);
      return () => {
        const i = stderrHandlers.indexOf(h);
        if (i >= 0) stderrHandlers.splice(i, 1);
      };
    },
    exited: proc.exited.then((code) => (code ?? null) as number | null),
  };
};
