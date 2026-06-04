// System-level operations: version, info, disk usage, prune-all, and the
// live event stream that drives the activity feed + auto-refresh.

import type {
  DiskUsage,
  DockerEvent,
  PruneReport,
  SystemInfo,
  SystemVersion,
} from "../../shared/types.ts";
import { json, ndjson, req } from "./client.ts";

export const ping = async (): Promise<boolean> => {
  try {
    await req("/_ping");
    return true;
  } catch {
    return false;
  }
};

export const version = async (): Promise<SystemVersion> => {
  const v = await json<{
    Version: string;
    ApiVersion: string;
    Os: string;
    Arch: string;
    KernelVersion: string;
    GoVersion: string;
    BuildTime: string;
  }>("/version");
  return {
    version: v.Version,
    apiVersion: v.ApiVersion,
    os: v.Os,
    arch: v.Arch,
    kernelVersion: v.KernelVersion,
    goVersion: v.GoVersion,
    buildTime: v.BuildTime,
  };
};

export const info = async (): Promise<SystemInfo> => {
  const i = await json<{
    Name: string;
    Containers: number;
    ContainersRunning: number;
    ContainersPaused: number;
    ContainersStopped: number;
    Images: number;
    ServerVersion: string;
    OperatingSystem: string;
    Architecture: string;
    NCPU: number;
    MemTotal: number;
    DockerRootDir: string;
  }>("/info");
  return {
    name: i.Name,
    containers: i.Containers,
    containersRunning: i.ContainersRunning,
    containersPaused: i.ContainersPaused,
    containersStopped: i.ContainersStopped,
    images: i.Images,
    serverVersion: i.ServerVersion,
    operatingSystem: i.OperatingSystem,
    architecture: i.Architecture,
    ncpu: i.NCPU,
    memTotal: i.MemTotal,
    dockerRootDir: i.DockerRootDir,
  };
};

type RawDf = {
  Images?: { Size?: number; Containers?: number }[] | null;
  Containers?: { SizeRw?: number }[] | null;
  Volumes?: { UsageData?: { Size?: number } | null }[] | null;
  BuildCache?: { Size?: number; InUse?: boolean }[] | null;
  LayersSize?: number;
};

export const df = async (): Promise<DiskUsage> => {
  const d = await json<RawDf>("/system/df");
  const images = d.Images ?? [];
  const containers = d.Containers ?? [];
  const volumes = d.Volumes ?? [];
  const buildCache = d.BuildCache ?? [];
  const sum = (ns: (number | undefined)[]): number => ns.reduce((a: number, b) => a + (b ?? 0), 0);

  const imageSize = d.LayersSize ?? sum(images.map((i) => i.Size));
  const imageActive = images.filter((i) => (i.Containers ?? 0) > 0);
  const imageActiveSize = sum(imageActive.map((i) => i.Size));
  const volSize = sum(volumes.map((v) => v.UsageData?.Size));
  const bcSize = sum(buildCache.map((b) => b.Size));
  const bcReclaimable = sum(buildCache.filter((b) => !b.InUse).map((b) => b.Size));

  return {
    images: {
      count: images.length,
      size: imageSize,
      reclaimable: Math.max(0, imageSize - imageActiveSize),
    },
    containers: {
      count: containers.length,
      size: sum(containers.map((c) => c.SizeRw)),
      reclaimable: 0,
    },
    volumes: { count: volumes.length, size: volSize, reclaimable: 0 },
    buildCache: { count: buildCache.length, size: bcSize, reclaimable: bcReclaimable },
  };
};

// Prune everything reclaimable, returning a per-kind report.
export const pruneAll = async (): Promise<PruneReport[]> => {
  const reports: PruneReport[] = [];
  const one = async (kind: string, path: string, key: string): Promise<void> => {
    try {
      const raw = await json<Record<string, unknown>>(path, { method: "POST" });
      const removedList = (raw[key] as unknown[] | null) ?? [];
      reports.push({
        kind,
        removed: Array.isArray(removedList) ? removedList.length : 0,
        reclaimed: (raw.SpaceReclaimed as number) ?? 0,
      });
    } catch {
      reports.push({ kind, removed: 0, reclaimed: 0 });
    }
  };
  await one("containers", "/containers/prune", "ContainersDeleted");
  await one("images", "/images/prune", "ImagesDeleted");
  await one("volumes", "/volumes/prune", "VolumesDeleted");
  await one("networks", "/networks/prune", "NetworksDeleted");
  await one("build cache", "/build/prune", "CachesDeleted");
  return reports;
};

type RawEvent = {
  Type?: string;
  Action?: string;
  Actor?: { ID?: string; Attributes?: Record<string, string> };
  time?: number;
  status?: string;
  id?: string;
  from?: string;
};

const formatEvent = (e: RawEvent): DockerEvent => {
  const type = e.Type ?? "";
  const action = e.Action ?? e.status ?? "";
  const attrs = e.Actor?.Attributes ?? {};
  const name = attrs.name ?? e.Actor?.ID ?? e.id ?? "";
  const short = name.length > 12 && /^[0-9a-f]+$/.test(name) ? name.slice(0, 12) : name;
  return {
    type,
    action,
    actor: short,
    time: e.time ?? 0,
    message: `${type} ${action} ${short}`.trim(),
  };
};

// Stream daemon events until `signal` aborts. `onEvent` fires per event.
export const streamEvents = async (
  signal: AbortSignal,
  onEvent: (e: DockerEvent) => void,
): Promise<void> => {
  for await (const raw of ndjson<RawEvent>("/events", { signal })) {
    onEvent(formatEvent(raw));
  }
};
