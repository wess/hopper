// Image operations: list/inspect/history/tag/remove/prune/search/pull.

import type {
  Image,
  ImageHistoryEntry,
  ImageSearchResult,
  PullProgress,
  PushProgress,
} from "../../shared/types.ts";
import { action, json, ndjson } from "./client.ts";
import { encodeRegistryAuth, registryHost, resolveAuth } from "./credentials.ts";

type RawImage = {
  Id: string;
  RepoTags?: string[] | null;
  RepoDigests?: string[] | null;
  Created: number;
  Size: number;
  Containers?: number;
  Labels?: Record<string, string> | null;
};

const mapImage = (i: RawImage): Image => {
  const repoTags = (i.RepoTags ?? []).filter((t) => t && t !== "<none>:<none>");
  return {
    id: i.Id,
    repoTags,
    repoDigests: i.RepoDigests ?? [],
    created: i.Created,
    size: i.Size,
    containers: i.Containers ?? -1,
    dangling: repoTags.length === 0,
    labels: i.Labels ?? {},
  };
};

export const list = async (all: boolean): Promise<Image[]> => {
  const raw = await json<RawImage[]>("/images/json", { query: { all, digests: true } });
  return raw.map(mapImage);
};

export const inspect = (id: string) => json<Record<string, unknown>>(`/images/${id}/json`);

export const history = async (id: string): Promise<ImageHistoryEntry[]> => {
  const raw = await json<
    { Id: string; Created: number; CreatedBy: string; Size: number; Comment: string }[]
  >(`/images/${id}/history`);
  return raw.map((h) => ({
    id: h.Id,
    created: h.Created,
    createdBy: h.CreatedBy,
    size: h.Size,
    comment: h.Comment,
  }));
};

export const remove = (id: string, force?: boolean) =>
  json<void>(`/images/${id}`, { method: "DELETE", query: { force } });

export const tag = (id: string, repo: string, tag: string) =>
  action(`/images/${id}/tag`, { query: { repo, tag } });

export const prune = async (all: boolean): Promise<{ removed: number; reclaimed: number }> => {
  const raw = await json<{ ImagesDeleted?: unknown[] | null; SpaceReclaimed?: number }>(
    "/images/prune",
    { method: "POST", query: { filters: JSON.stringify({ dangling: { [String(!all)]: true } }) } },
  );
  return { removed: (raw.ImagesDeleted ?? []).length, reclaimed: raw.SpaceReclaimed ?? 0 };
};

export const search = async (term: string): Promise<ImageSearchResult[]> => {
  const raw = await json<
    {
      name: string;
      description: string;
      star_count: number;
      is_official: boolean;
      is_automated: boolean;
    }[]
  >("/images/search", { query: { term, limit: 25 } });
  return raw.map((r) => ({
    name: r.name,
    description: r.description,
    stars: r.star_count,
    official: r.is_official,
    automated: r.is_automated,
  }));
};

type RawPull = {
  id?: string;
  status?: string;
  error?: string;
  progressDetail?: { current?: number; total?: number };
};

// Pull an image, invoking `onProgress` per status frame. Normalizes the
// daemon's progress stream into `PullProgress`. Resolves with the final
// error (if any) so the handler can report ok/fail.
export const pull = async (
  requestId: string,
  ref: string,
  onProgress: (p: PullProgress) => void,
): Promise<{ ok: boolean; error?: string }> => {
  // Default to :latest when no tag/digest is given.
  const fromImage = ref.includes(":") || ref.includes("@") ? ref : `${ref}:latest`;
  let error: string | undefined;
  for await (const frame of ndjson<RawPull>("/images/create", {
    method: "POST",
    query: { fromImage },
  })) {
    if (frame.error) {
      error = frame.error;
      onProgress({ requestId, status: frame.error, done: true, error: frame.error });
      continue;
    }
    onProgress({
      requestId,
      id: frame.id,
      status: frame.status ?? "",
      current: frame.progressDetail?.current,
      total: frame.progressDetail?.total,
      done: false,
    });
  }
  onProgress({ requestId, status: error ?? "Pull complete", done: true, error });
  return { ok: !error, error };
};

// Split "registry/name:tag" into its push name (no tag) and tag. The tag is
// the part after the last colon, but only when that colon follows the last
// slash (so a `host:port/name` registry port isn't mistaken for a tag).
export const splitRef = (ref: string): { name: string; tag: string } => {
  const lastSlash = ref.lastIndexOf("/");
  const lastColon = ref.lastIndexOf(":");
  if (lastColon > lastSlash)
    return { name: ref.slice(0, lastColon), tag: ref.slice(lastColon + 1) };
  return { name: ref, tag: "latest" };
};

// Push an image to its registry, streaming progress like `pull`. Credentials
// come from the user's docker config; the daemon requires an `X-Registry-Auth`
// header even for an anonymous push, so we always send one.
export const push = async (
  requestId: string,
  ref: string,
  onProgress: (p: PushProgress) => void,
): Promise<{ ok: boolean; error?: string }> => {
  const { name, tag } = splitRef(ref);
  const auth = (await resolveAuth(ref)) ?? { serveraddress: registryHost(ref) };
  let error: string | undefined;
  for await (const frame of ndjson<RawPull>(`/images/${name}/push`, {
    method: "POST",
    query: { tag },
    headers: { "X-Registry-Auth": encodeRegistryAuth(auth) },
  })) {
    if (frame.error) {
      error = frame.error;
      onProgress({ requestId, status: frame.error, done: true, error: frame.error });
      continue;
    }
    onProgress({
      requestId,
      id: frame.id,
      status: frame.status ?? "",
      current: frame.progressDetail?.current,
      total: frame.progressDetail?.total,
      done: false,
    });
  }
  onProgress({ requestId, status: error ?? "Push complete", done: true, error });
  return { ok: !error, error };
};
