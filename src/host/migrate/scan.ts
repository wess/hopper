// Find a source engine to migrate FROM (a reachable Docker daemon that isn't
// Hopper's own active engine — typically Docker Desktop) and enumerate what it
// holds, projected into selectable MigrationItems.

import { realpathSync } from "node:fs";
import type { MigrationItem, MigrationScan } from "../../shared/types.ts";
import { currentEndpoint, describeEndpoint, type Endpoint } from "../docker/endpoint.ts";
import { probe } from "../engine/diagnose.ts";
import { candidateSockets } from "../engine/providers/existing.ts";
import { engineStatus } from "../engine/registry.ts";
import { PREDEFINED_NETWORKS } from "./helpers.ts";
import { jsonOn } from "./transport.ts";

const fmtSize = (n: number): string => {
  if (!n) return "";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
};

// Canonicalize a unix socket path so symlink aliases compare equal — on macOS
// Docker Desktop makes /var/run/docker.sock a symlink to ~/.docker/run/docker.sock,
// the same daemon under two path strings.
const canonUnix = (p: string): string => {
  try {
    return realpathSync(p);
  } catch {
    return p;
  }
};

// Do two endpoints address the same daemon? Transport-aware, alias-safe.
const sameDaemon = (a: Endpoint, b: Endpoint): boolean => {
  if (a.transport === "tcp" || b.transport === "tcp") {
    return a.transport === "tcp" && b.transport === "tcp" && a.host === b.host && a.port === b.port;
  }
  return canonUnix(a.path) === canonUnix(b.path);
};

// The first reachable external socket that isn't the destination — the engine
// the user is leaving (Docker Desktop, Colima, …).
const findSource = async (dest: Endpoint): Promise<Endpoint | null> => {
  for (const ep of candidateSockets()) {
    if (sameDaemon(ep, dest)) continue;
    if ((await probe(ep)).ok) return ep;
  }
  return null;
};

// Resolve both ends of a migration. The destination must be Hopper's own
// (managed) engine, running — otherwise we'd "migrate" into Docker Desktop
// itself. When `pinned` is given (the source the UI scanned), use exactly that
// engine so a daemon coming up/down between scan and run can't switch sources.
// Returns an error string instead of endpoints when prerequisites aren't met.
export type Ends = { source: Endpoint; dest: Endpoint } | { error: string };

export const resolveEnds = async (pinned?: Endpoint): Promise<Ends> => {
  const status = await engineStatus();
  if (!status.managed || !status.connected) {
    return {
      error:
        "Start Hopper's own engine first — it must be running to receive the migration (Settings → Engine → Start).",
    };
  }
  const dest = currentEndpoint();

  if (pinned) {
    if (sameDaemon(pinned, dest)) {
      return { error: "The scanned source is Hopper's own engine — nothing to migrate from." };
    }
    if (!(await probe(pinned)).ok) {
      return {
        error: `The source engine you scanned (${describeEndpoint(pinned)}) is no longer reachable — rescan before migrating.`,
      };
    }
    return { source: pinned, dest };
  }

  const source = await findSource(dest);
  if (!source) {
    return {
      error: "No other Docker engine found to migrate from. Start Docker Desktop and retry.",
    };
  }
  return { source, dest };
};

type RawContainer = { Id: string; Names?: string[]; Image: string; State: string; Status: string };
type RawImage = { Id: string; RepoTags?: string[] | null; Size?: number };
type RawVolume = { Name: string; Driver: string };
type RawNetwork = { Id: string; Name: string; Driver: string };

export const scan = async (): Promise<MigrationScan> => {
  const ends = await resolveEnds();
  if ("error" in ends) {
    return {
      available: false,
      containers: [],
      images: [],
      volumes: [],
      networks: [],
      message: ends.error,
    };
  }
  const { source } = ends;

  const containersRaw = await jsonOn<RawContainer[]>(source, "/containers/json", {
    query: { all: true },
  });
  const containers: MigrationItem[] = containersRaw.map((c) => ({
    kind: "container",
    id: c.Id,
    name: (c.Names?.[0] ?? c.Id).replace(/^\//, ""),
    detail: `${c.State} · ${c.Image}`,
  }));

  const imagesRaw = await jsonOn<RawImage[]>(source, "/images/json", {});
  const images: MigrationItem[] = imagesRaw
    .filter((i) => i.RepoTags?.some((t) => t && t !== "<none>:<none>"))
    .map((i) => {
      const tag = (i.RepoTags ?? []).find((t) => t && t !== "<none>:<none>") ?? i.Id;
      return { kind: "image", id: i.Id, name: tag, detail: fmtSize(i.Size ?? 0) };
    });

  const volRaw = await jsonOn<{ Volumes?: RawVolume[] | null }>(source, "/volumes", {});
  const volumes: MigrationItem[] = (volRaw.Volumes ?? []).map((v) => ({
    kind: "volume",
    id: v.Name,
    name: v.Name,
    detail: v.Driver,
  }));

  const netRaw = await jsonOn<RawNetwork[]>(source, "/networks", {});
  const networks: MigrationItem[] = netRaw
    .filter((n) => !PREDEFINED_NETWORKS.has(n.Name))
    .map((n) => ({ kind: "network", id: n.Id, name: n.Name, detail: n.Driver }));

  return {
    available: true,
    source: describeEndpoint(source),
    sourceEndpoint: source,
    containers,
    images,
    volumes,
    networks,
  };
};
