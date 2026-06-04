// Container operations against the Docker Engine API, mapping raw responses
// into the lean `Container` shape the UI consumes.

import type {
  Container,
  ContainerState,
  Mount,
  Port,
  ProcessList,
  RunInput,
} from "../../shared/types.ts";
import { action, json, type Query, req } from "./client.ts";

type RawPort = { IP?: string; PrivatePort: number; PublicPort?: number; Type: string };
type RawMount = {
  Type: string;
  Source: string;
  Destination: string;
  Mode: string;
  RW: boolean;
  Name?: string;
};
type RawContainer = {
  Id: string;
  Names: string[];
  Image: string;
  ImageID: string;
  Command: string;
  Created: number;
  State: string;
  Status: string;
  Ports?: RawPort[];
  Labels?: Record<string, string>;
  Mounts?: RawMount[];
  NetworkSettings?: { Networks?: Record<string, unknown> };
};

const normalizeState = (s: string): ContainerState => {
  const v = s.toLowerCase();
  if (
    v === "running" ||
    v === "paused" ||
    v === "restarting" ||
    v === "removing" ||
    v === "exited" ||
    v === "dead" ||
    v === "created"
  ) {
    return v;
  }
  return "exited";
};

const mapPort = (p: RawPort): Port => ({
  ip: p.IP,
  privatePort: p.PrivatePort,
  publicPort: p.PublicPort,
  type: p.Type,
});

const mapMount = (m: RawMount): Mount => ({
  type: m.Type,
  source: m.Source,
  destination: m.Destination,
  mode: m.Mode,
  rw: m.RW,
  name: m.Name,
});

const mapContainer = (c: RawContainer): Container => {
  const labels = c.Labels ?? {};
  const name = (c.Names[0] ?? "").replace(/^\//, "");
  return {
    id: c.Id,
    name,
    image: c.Image,
    imageId: c.ImageID,
    command: c.Command,
    created: c.Created,
    state: normalizeState(c.State),
    status: c.Status,
    ports: (c.Ports ?? []).map(mapPort),
    labels,
    mounts: (c.Mounts ?? []).map(mapMount),
    networks: Object.keys(c.NetworkSettings?.Networks ?? {}),
    composeProject: labels["com.docker.compose.project"] ?? null,
    composeService: labels["com.docker.compose.service"] ?? null,
  };
};

export const list = async (all: boolean): Promise<Container[]> => {
  const raw = await json<RawContainer[]>("/containers/json", { query: { all } });
  return raw.map(mapContainer);
};

export const inspect = (id: string) => json<Record<string, unknown>>(`/containers/${id}/json`);

export const start = (id: string) => action(`/containers/${id}/start`);
export const stop = (id: string) => action(`/containers/${id}/stop`, { query: { t: 10 } });
export const restart = (id: string) => action(`/containers/${id}/restart`, { query: { t: 10 } });
export const pause = (id: string) => action(`/containers/${id}/pause`);
export const unpause = (id: string) => action(`/containers/${id}/unpause`);
export const kill = (id: string) => action(`/containers/${id}/kill`);
export const rename = (id: string, name: string) =>
  action(`/containers/${id}/rename`, { query: { name } });

export const remove = (id: string, force?: boolean, volumes?: boolean) =>
  json<void>(`/containers/${id}`, { method: "DELETE", query: { force, v: volumes } });

export const top = async (id: string): Promise<ProcessList> => {
  const raw = await json<{ Titles: string[]; Processes: string[][] }>(`/containers/${id}/top`, {
    query: { ps_args: "aux" },
  });
  return { titles: raw.Titles ?? [], processes: raw.Processes ?? [] };
};

export const prune = async (): Promise<{ removed: number; reclaimed: number }> => {
  const raw = await json<{ ContainersDeleted?: string[] | null; SpaceReclaimed?: number }>(
    "/containers/prune",
    { method: "POST" },
  );
  return { removed: (raw.ContainersDeleted ?? []).length, reclaimed: raw.SpaceReclaimed ?? 0 };
};

// Create + start a container from the Run dialog input.
export const run = async (input: RunInput): Promise<{ id: string }> => {
  const exposedPorts: Record<string, Record<string, never>> = {};
  const portBindings: Record<string, { HostPort: string }[]> = {};
  for (const p of input.ports ?? []) {
    const key = `${p.container}/${p.proto ?? "tcp"}`;
    exposedPorts[key] = {};
    portBindings[key] = [{ HostPort: p.host }];
  }

  const binds = (input.volumes ?? []).map((v) => `${v.host}:${v.container}${v.ro ? ":ro" : ""}`);

  const body: Record<string, unknown> = {
    Image: input.image,
    Env: input.env ?? [],
    ExposedPorts: exposedPorts,
    HostConfig: {
      PortBindings: portBindings,
      Binds: binds,
      RestartPolicy: input.restart ? { Name: input.restart } : undefined,
      AutoRemove: input.autoRemove ?? false,
    },
  };
  if (input.command?.trim()) {
    // naive split is fine for the dialog; advanced users use compose
    body.Cmd = input.command.trim().split(/\s+/);
  }

  const query: Query = input.name ? { name: input.name } : {};
  const created = await json<{ Id: string }>("/containers/create", {
    method: "POST",
    query,
    body,
  });
  await start(created.Id);
  return { id: created.Id };
};

// Logs path builder — used by the streaming layer in logs.ts.
export const logsPath = (id: string): string => `/containers/${id}/logs`;

// Lightweight existence/probe used by exec + log streamers.
export const exists = async (id: string): Promise<boolean> => {
  try {
    await req(`/containers/${id}/json`);
    return true;
  } catch {
    return false;
  }
};
