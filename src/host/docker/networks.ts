// Network operations: list/inspect/create/remove/prune/connect/disconnect.

import type { Network, NetworkCreateInput } from "../../shared/types.ts";
import { action, json } from "./client.ts";

type RawNetwork = {
  Id: string;
  Name: string;
  Driver: string;
  Scope: string;
  Internal: boolean;
  Attachable: boolean;
  Created?: string;
  Labels?: Record<string, string> | null;
  IPAM?: { Config?: { Subnet?: string; Gateway?: string }[] | null } | null;
  Containers?: Record<string, unknown> | null;
};

const mapNetwork = (n: RawNetwork): Network => ({
  id: n.Id,
  name: n.Name,
  driver: n.Driver,
  scope: n.Scope,
  internal: n.Internal,
  attachable: n.Attachable,
  ipam: (n.IPAM?.Config ?? []).map((c) => ({ subnet: c.Subnet, gateway: c.Gateway })),
  containers: Object.keys(n.Containers ?? {}).length,
  created: n.Created ?? "",
  labels: n.Labels ?? {},
});

export const list = async (): Promise<Network[]> => {
  // The list endpoint omits Containers; that's fine — inspect fills it in.
  const raw = await json<RawNetwork[]>("/networks");
  return raw.map(mapNetwork);
};

export const inspect = (id: string) => json<Record<string, unknown>>(`/networks/${id}`);

export const create = async (input: NetworkCreateInput): Promise<Network> => {
  const body: Record<string, unknown> = {
    Name: input.name,
    Driver: input.driver ?? "bridge",
    Internal: input.internal ?? false,
    Attachable: input.attachable ?? false,
  };
  if (input.subnet || input.gateway) {
    body.IPAM = { Config: [{ Subnet: input.subnet, Gateway: input.gateway }] };
  }
  const created = await json<{ Id: string }>("/networks/create", { method: "POST", body });
  const full = await inspect(created.Id);
  return mapNetwork(full as RawNetwork);
};

export const remove = (id: string) => json<void>(`/networks/${id}`, { method: "DELETE" });

export const prune = async (): Promise<{ removed: number; reclaimed: number }> => {
  const raw = await json<{ NetworksDeleted?: string[] | null }>("/networks/prune", {
    method: "POST",
  });
  return { removed: (raw.NetworksDeleted ?? []).length, reclaimed: 0 };
};

export const connect = (id: string, container: string) =>
  action(`/networks/${id}/connect`, { body: { Container: container } });

export const disconnect = (id: string, container: string, force?: boolean) =>
  action(`/networks/${id}/disconnect`, { body: { Container: container, Force: force ?? false } });
