// Volume operations. List enriches each volume with size (from `system df`)
// and in-use state (from the volumes list `UsageData` / ref counts).

import type { Volume } from "../../shared/types.ts";
import { json } from "./client.ts";

type RawVolume = {
  Name: string;
  Driver: string;
  Mountpoint: string;
  CreatedAt?: string;
  Scope: string;
  Labels?: Record<string, string> | null;
  UsageData?: { Size?: number; RefCount?: number } | null;
};

const mapVolume = (v: RawVolume): Volume => {
  const usage = v.UsageData;
  return {
    name: v.Name,
    driver: v.Driver,
    mountpoint: v.Mountpoint,
    created: v.CreatedAt ?? "",
    scope: v.Scope,
    labels: v.Labels ?? {},
    size: usage?.Size ?? -1,
    inUse: (usage?.RefCount ?? 0) > 0,
  };
};

export const list = async (): Promise<Volume[]> => {
  const raw = await json<{ Volumes: RawVolume[] | null }>("/volumes");
  return (raw.Volumes ?? []).map(mapVolume);
};

export const inspect = (name: string) =>
  json<Record<string, unknown>>(`/volumes/${encodeURIComponent(name)}`);

export const create = async (name: string, driver?: string): Promise<Volume> => {
  const raw = await json<RawVolume>("/volumes/create", {
    method: "POST",
    body: { Name: name, Driver: driver ?? "local" },
  });
  return mapVolume(raw);
};

export const remove = (name: string, force?: boolean) =>
  json<void>(`/volumes/${encodeURIComponent(name)}`, { method: "DELETE", query: { force } });

export const prune = async (): Promise<{ removed: number; reclaimed: number }> => {
  const raw = await json<{ VolumesDeleted?: string[] | null; SpaceReclaimed?: number }>(
    "/volumes/prune",
    { method: "POST" },
  );
  return { removed: (raw.VolumesDeleted ?? []).length, reclaimed: raw.SpaceReclaimed ?? 0 };
};
