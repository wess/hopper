// Network migration: recreate user-defined networks from their source inspect.
// ensureNetwork is reused by container recreate to materialize a network a
// container references even if the user didn't explicitly select it.

import type { Endpoint } from "../docker/endpoint.ts";
import { type Emit, type NetworkInspect, PREDEFINED_NETWORKS, step } from "./helpers.ts";
import { jsonOn } from "./transport.ts";

const existsOnDest = async (dest: Endpoint, name: string): Promise<boolean> => {
  const existing = await jsonOn<{ Name: string }[]>(dest, "/networks", {
    query: { filters: JSON.stringify({ name: [name] }) },
  });
  return existing.some((e) => e.Name === name);
};

const createFrom = async (dest: Endpoint, n: NetworkInspect): Promise<void> => {
  await jsonOn(dest, "/networks/create", {
    method: "POST",
    body: {
      Name: n.Name,
      Driver: n.Driver,
      Internal: n.Internal,
      Attachable: n.Attachable,
      EnableIPv6: n.EnableIPv6,
      Options: n.Options,
      Labels: n.Labels,
      IPAM: n.IPAM ? { Driver: n.IPAM.Driver, Config: n.IPAM.Config } : undefined,
    },
  });
};

// Make sure a network (by name or id) exists on dest, recreating it from the
// source if needed. Predefined networks (bridge/host/none) are no-ops. Idempotent.
export const ensureNetwork = async (
  source: Endpoint,
  dest: Endpoint,
  nameOrId: string,
): Promise<void> => {
  const n = await jsonOn<NetworkInspect>(source, `/networks/${nameOrId}`);
  if (PREDEFINED_NETWORKS.has(n.Name)) return;
  if (await existsOnDest(dest, n.Name)) return;
  await createFrom(dest, n);
};

export const migrateNetworks = async (
  source: Endpoint,
  dest: Endpoint,
  ids: readonly string[],
  emit: Emit,
): Promise<void> => {
  let done = 0;
  for (const id of ids) {
    let name = id;
    try {
      const n = await jsonOn<NetworkInspect>(source, `/networks/${id}`);
      name = n.Name;
      step(emit, "networks", name, done, ids.length, `Recreating network ${name}`);
      if (!PREDEFINED_NETWORKS.has(name) && !(await existsOnDest(dest, name))) {
        await createFrom(dest, n);
      }
    } catch (e) {
      step(emit, "networks", name, done, ids.length, `Network ${name} failed`, {
        error: String(e),
      });
    }
    done++;
  }
};
