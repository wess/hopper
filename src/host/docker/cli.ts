import type { DockerCliSetupResult, DockerCliStatus } from "../../shared/types.ts";
import { currentEndpoint, dockerHostValue } from "./endpoint.ts";

const CONTEXT = "hopper";

type CommandResult = {
  readonly ok: boolean;
  readonly code: number;
  readonly stdout: string;
  readonly stderr: string;
};

type Runner = (args: readonly string[]) => Promise<CommandResult>;

const text = async (stream: ReadableStream<Uint8Array> | null): Promise<string> =>
  stream ? new Response(stream).text() : "";

const run: Runner = async (args) => {
  try {
    const proc = Bun.spawn(["docker", ...args], { stdout: "pipe", stderr: "pipe" });
    const [stdout, stderr] = await Promise.all([text(proc.stdout), text(proc.stderr)]);
    const code = await proc.exited;
    return { ok: code === 0, code, stdout, stderr };
  } catch (e) {
    return {
      ok: false,
      code: 127,
      stdout: "",
      stderr: e instanceof Error ? e.message : String(e),
    };
  }
};

export const contextCommands = (host: string, exists: boolean): readonly (readonly string[])[] => [
  exists
    ? ["context", "update", CONTEXT, "--docker", `host=${host}`]
    : [
        "context",
        "create",
        CONTEXT,
        "--description",
        "Hopper managed Docker engine",
        "--docker",
        `host=${host}`,
      ],
  ["context", "use", CONTEXT],
];

const detail = (result: CommandResult): string =>
  (result.stderr || result.stdout || `docker exited ${result.code}`).trim();

export const status = async (runner: Runner = run): Promise<DockerCliStatus> => {
  const host = dockerHostValue(currentEndpoint());
  const version = await runner(["--version"]);
  if (!version.ok) {
    return {
      available: false,
      configured: false,
      host,
      detail: "Docker CLI was not found on PATH.",
    };
  }

  const current = await runner(["context", "show"]);
  const context = current.ok ? current.stdout.trim() : undefined;
  return {
    available: true,
    configured: context === CONTEXT,
    host,
    context,
    detail: context === CONTEXT ? "Docker CLI is using Hopper." : "Docker CLI is not using Hopper.",
  };
};

export const setup = async (runner: Runner = run): Promise<DockerCliSetupResult> => {
  const version = await runner(["--version"]);
  if (!version.ok) {
    const s = await status(runner);
    return { ok: false, detail: s.detail, status: s };
  }

  const host = dockerHostValue(currentEndpoint());
  const exists = (await runner(["context", "inspect", CONTEXT])).ok;
  for (const args of contextCommands(host, exists)) {
    const result = await runner(args);
    if (!result.ok) {
      const s = await status(runner);
      return { ok: false, detail: detail(result), status: s };
    }
  }

  return {
    ok: true,
    detail: `Docker CLI now targets ${host}.`,
    status: await status(runner),
  };
};
