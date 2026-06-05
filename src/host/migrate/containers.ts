// Container migration: recreate each selected container on the dest from its
// source inspect, left STOPPED so the user starts it deliberately. We preserve
// all network attachments (not just the primary), sanitize daemon-specific
// HostConfig fields, remap container:<id> network modes, skip names that already
// exist, and surface non-fatal advisories (host-path binds, arch mismatch).

import type { Endpoint } from "../docker/endpoint.ts";
import {
  type ContainerInspect,
  type Emit,
  type EndpointSettings,
  PREDEFINED_NETMODES,
  PREDEFINED_NETWORKS,
  step,
} from "./helpers.ts";
import { ensureImage } from "./images.ts";
import { ensureNetwork } from "./networks.ts";
import { jsonOn } from "./transport.ts";

// Reset/clear HostConfig fields meaningful on the source daemon but liable to
// make /containers/create fail on Hopper's VM.
const sanitizeHostConfig = (hc: Record<string, unknown> | undefined): Record<string, unknown> => {
  const out = { ...(hc ?? {}) };
  out.LogConfig = { Type: "", Config: {} }; // let the dest pick its default log driver
  delete out.Runtime; // a source-specific runtime won't exist on the dest
  for (const k of ["IpcMode", "PidMode", "CgroupnsMode", "UsernsMode"]) {
    const v = out[k];
    if (typeof v === "string" && v.startsWith("container:")) delete out[k];
  }
  return out;
};

// Keep only the endpoint settings worth carrying over (aliases drive DNS
// discovery between containers; static IP / links / mac if present).
const endpointConfig = (e: EndpointSettings | undefined): Record<string, unknown> | undefined => {
  if (!e) return undefined;
  const cfg: Record<string, unknown> = {};
  if (e.Aliases?.length) cfg.Aliases = e.Aliases;
  if (e.IPAMConfig) cfg.IPAMConfig = e.IPAMConfig;
  if (e.Links?.length) cfg.Links = e.Links;
  if (e.MacAddress) cfg.MacAddress = e.MacAddress;
  return Object.keys(cfg).length ? cfg : undefined;
};

const containerExistsOnDest = async (dest: Endpoint, name: string): Promise<boolean> => {
  const existing = await jsonOn<{ Names?: string[] }[]>(dest, "/containers/json", {
    query: { all: true, filters: JSON.stringify({ name: [`^/${name}$`] }) },
  });
  return existing.some((c) => (c.Names ?? []).some((n) => n.replace(/^\//, "") === name));
};

// Resolve NetworkMode for the dest: remap container:<id> via the id-map (or fall
// back to default), ensure a named user network exists. Predefined modes pass through.
const resolveNetworkMode = async (
  source: Endpoint,
  dest: Endpoint,
  mode: unknown,
  idMap: Map<string, string>,
): Promise<string> => {
  const m = typeof mode === "string" ? mode : "";
  if (m.startsWith("container:")) {
    const destId = idMap.get(m.slice("container:".length));
    return destId ? `container:${destId}` : "default"; // referenced container not migrated
  }
  if (PREDEFINED_NETMODES.has(m)) return m;
  await ensureNetwork(source, dest, m); // a named user network — materialize it first
  return m;
};

// docker /info reports arch as x86_64/aarch64; image inspect uses amd64/arm64.
const normArch = (a: string): string =>
  ({ x86_64: "amd64", aarch64: "arm64", armv7l: "arm" })[a] ?? a;

export const migrateContainers = async (
  source: Endpoint,
  dest: Endpoint,
  ids: readonly string[],
  emit: Emit,
): Promise<void> => {
  const idMap = new Map<string, string>(); // source id -> dest created id (for container: refs)
  let engineArch: string | undefined;
  try {
    engineArch = (await jsonOn<{ Architecture?: string }>(dest, "/info")).Architecture;
  } catch {
    // arch advisory is best-effort
  }

  let done = 0;
  for (const id of ids) {
    let name = id.slice(0, 12);
    try {
      const insp = await jsonOn<ContainerInspect>(source, `/containers/${id}/json`);
      name = (insp.Name ?? "").replace(/^\//, "") || name;
      step(emit, "containers", name, done, ids.length, `Recreating container ${name}`);

      const image = insp.Config?.Image;
      if (!image) throw new Error("container has no image reference");
      await ensureImage(source, dest, image);

      if (await containerExistsOnDest(dest, name)) {
        step(emit, "containers", name, done, ids.length, `Container ${name} already present`, {
          warning: `${name} already exists on the destination — skipped`,
        });
        done++;
        continue;
      }

      const hc = sanitizeHostConfig(insp.HostConfig);
      hc.NetworkMode = await resolveNetworkMode(source, dest, hc.NetworkMode, idMap);

      // Attach the primary network at create (the API allows only one there);
      // connect the rest afterward so multi-network containers keep every
      // attachment (and their aliases for DNS discovery).
      const networks = insp.NetworkSettings?.Networks ?? {};
      const userNets = Object.keys(networks).filter((n) => !PREDEFINED_NETWORKS.has(n));
      const primary =
        typeof hc.NetworkMode === "string" && userNets.includes(hc.NetworkMode)
          ? hc.NetworkMode
          : userNets[0];
      if (primary) hc.NetworkMode = primary; // keep NetworkMode and the endpoint in agreement

      const body: Record<string, unknown> = { ...insp.Config, HostConfig: hc };
      if (primary) {
        body.NetworkingConfig = {
          EndpointsConfig: { [primary]: endpointConfig(networks[primary]) ?? {} },
        };
      }

      const created = await jsonOn<{ Id: string }>(dest, "/containers/create", {
        method: "POST",
        query: { name },
        body,
      });
      idMap.set(id, created.Id);

      for (const net of userNets) {
        if (net === primary) continue;
        try {
          await ensureNetwork(source, dest, net);
          await jsonOn(dest, `/networks/${net}/connect`, {
            method: "POST",
            body: { Container: created.Id, EndpointConfig: endpointConfig(networks[net]) ?? {} },
          });
        } catch (e) {
          step(emit, "containers", name, done, ids.length, `Network ${net} not attached`, {
            warning: `${name}: couldn't attach to network ${net} — ${String(e)}`,
          });
        }
      }

      // Non-fatal advisories the user should see before starting.
      const binds = (insp.HostConfig?.Binds as string[] | undefined) ?? [];
      const hostBind = binds.find((b) => b.startsWith("/") || b.startsWith("~"));
      if (hostBind) {
        step(emit, "containers", name, done, ids.length, `Recreated ${name}`, {
          warning: `${name}: bind "${hostBind}" points at a host path that may not exist in Hopper's VM — reconcile before starting`,
        });
      }
      if (engineArch) {
        try {
          const img = await jsonOn<{ Architecture?: string }>(dest, `/images/${image}/json`);
          if (img.Architecture && normArch(img.Architecture) !== normArch(engineArch)) {
            step(emit, "containers", name, done, ids.length, `Recreated ${name}`, {
              warning: `${name}: image is ${img.Architecture} but the engine is ${engineArch} — may need emulation to start`,
            });
          }
        } catch {
          // arch advisory is best-effort
        }
      }
    } catch (e) {
      step(emit, "containers", name, done, ids.length, `Container ${name} failed`, {
        error: String(e),
      });
    }
    done++;
  }
};
