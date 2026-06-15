// Image build against the classic (non-BuildKit) `/build` endpoint. Packs the
// context directory into a tar stream — honoring `.dockerignore` — and POSTs
// it, demuxing the daemon's JSON build log into `BuildProgress` frames.

import { join } from "node:path";
import type { BuildInput, BuildProgress } from "../../shared/types.ts";
import { ndjson, type Query } from "./client.ts";

// Translate a build request into the `/build` query string. Build args travel
// as a JSON-encoded object; `nocache`/`pull` are booleans.
export const buildQuery = (input: BuildInput): Query => {
  const q: Query = { dockerfile: input.dockerfile?.trim() || "Dockerfile", rm: true };
  if (input.tag?.trim()) q.t = input.tag.trim();
  if (input.target?.trim()) q.target = input.target.trim();
  if (input.noCache) q.nocache = true;
  if (input.pull) q.pull = true;
  if (input.buildArgs && Object.keys(input.buildArgs).length > 0) {
    q.buildargs = JSON.stringify(input.buildArgs);
  }
  return q;
};

// Parse a `.dockerignore` into tar exclude patterns. Comments, blanks, and
// negations (`!`) are dropped — negation needs allow-list semantics tar can't
// express, so those paths are simply included (matching is best-effort).
export const dockerignoreExcludes = (text: string): string[] =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith("#") && !l.startsWith("!"));

// Negation rules (`!pattern`) found in a `.dockerignore`. The classic builder
// packs the context with `tar --exclude`, which can't express "exclude X but
// keep Y", so these are NOT applied. We surface them to the user (see
// buildImage) rather than dropping them silently.
export const dockerignoreNegations = (text: string): string[] =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l.startsWith("!") && l.length > 1);

type RawBuildFrame = {
  stream?: string;
  status?: string;
  error?: string;
  errorDetail?: { message?: string };
  aux?: { ID?: string };
};

// Map one daemon build frame to a `BuildProgress` (non-terminal).
export const mapBuildFrame = (requestId: string, f: RawBuildFrame): BuildProgress => ({
  requestId,
  stream: f.stream,
  status: f.status,
  imageId: f.aux?.ID,
  error: f.error ?? f.errorDetail?.message,
  done: false,
});

const readDockerignore = async (
  dir: string,
): Promise<{ excludes: string[]; negations: string[] }> => {
  const file = Bun.file(join(dir, ".dockerignore"));
  if (!(await file.exists())) return { excludes: [], negations: [] };
  const text = await file.text();
  return { excludes: dockerignoreExcludes(text), negations: dockerignoreNegations(text) };
};

// Stream the context dir as a tar archive. `tar` writes to stdout, which we
// hand to fetch as a streaming request body.
const tarContext = (contextDir: string, excludes: string[]) => {
  const args = ["tar", "-cf", "-", "-C", contextDir];
  for (const e of excludes) args.push(`--exclude=${e}`);
  args.push(".");
  return Bun.spawn(args, { stdout: "pipe", stderr: "ignore" });
};

// Build an image, invoking `onProgress` per build-log frame. Resolves with the
// built image id (or an error) so the handler can report ok/fail.
export const buildImage = async (
  requestId: string,
  input: BuildInput,
  onProgress: (p: BuildProgress) => void,
): Promise<{ ok: boolean; error?: string; imageId?: string }> => {
  const { excludes, negations } = await readDockerignore(input.contextDir);
  const proc = tarContext(input.contextDir, excludes);
  let error: string | undefined;
  let imageId: string | undefined;
  // Surface unapplied negation rules so a kept-by-`!` file silently shipping
  // in the context isn't a mystery (the classic builder's tar can't honor them).
  if (negations.length > 0) {
    onProgress({
      requestId,
      status: `Note: ${negations.length} .dockerignore negation rule(s) (e.g. "${negations[0]}") are not applied by the classic builder — those paths follow their broader ignore rule.`,
      done: false,
    });
  }
  try {
    for await (const frame of ndjson<RawBuildFrame>("/build", {
      method: "POST",
      query: buildQuery(input),
      headers: { "Content-Type": "application/x-tar" },
      body: proc.stdout,
    })) {
      const p = mapBuildFrame(requestId, frame);
      if (p.error) error = p.error;
      if (p.imageId) imageId = p.imageId;
      onProgress(p);
    }
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  } finally {
    proc.kill();
  }
  onProgress({ requestId, status: error ? "Build failed" : "Build complete", done: true, error });
  return { ok: !error, error, imageId };
};
