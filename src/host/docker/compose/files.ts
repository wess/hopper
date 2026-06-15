// Compose file validation + read/write for the in-app editor. Validation defers
// to `docker compose config`, which parses, merges, and normalizes the file set
// exactly as a real `up` would — so the editor catches errors before launching.

import type { ComposeConfigResult, ComposeFileResult } from "../../../shared/types.ts";
import { childEnv, resolveRunner, toCommand } from "./runner.ts";

export const validateConfig = async (
  files: readonly string[],
  project?: string,
): Promise<ComposeConfigResult> => {
  const runner = await resolveRunner();
  if (!runner) return { ok: false, error: "Docker Compose is not available." };

  const args = ["compose"];
  for (const f of files) {
    if (f.trim()) args.push("-f", f.trim());
  }
  if (project?.trim()) args.push("-p", project.trim());
  args.push("config");

  try {
    const proc = Bun.spawn(toCommand(runner, args), {
      stdout: "pipe",
      stderr: "pipe",
      env: childEnv(),
    });
    const [stdout, stderr] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
    ]);
    await proc.exited;
    if (proc.exitCode === 0) return { ok: true, yaml: stdout };
    return {
      ok: false,
      error: stderr.trim() || `docker compose config exited with code ${proc.exitCode}`,
    };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};

export const readComposeFile = async (path: string): Promise<ComposeFileResult> => {
  if (!path.trim()) return { ok: false, error: "No file path given." };
  try {
    const file = Bun.file(path);
    if (!(await file.exists())) return { ok: false, error: `File not found: ${path}` };
    return { ok: true, content: await file.text() };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};

export const writeComposeFile = async (
  path: string,
  content: string,
): Promise<ComposeFileResult> => {
  if (!path.trim()) return { ok: false, error: "No file path given." };
  try {
    await Bun.write(path, content);
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};
