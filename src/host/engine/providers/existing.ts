// Connect to a Docker engine someone else is running.
//
// This is Hopper's original behaviour kept as a first-class provider and the
// universal fallback: it honours DOCKER_HOST / DOCKER_SOCKET, then sweeps the
// well-known socket locations so it finds Colima, OrbStack, Rancher, rootless
// dockerd, or a Podman compat socket without the user configuring anything.

import { homedir } from "node:os";
import { describeEndpoint, type Endpoint, resolveEndpoint } from "../../docker/endpoint.ts";
import { probe, toState } from "../diagnose.ts";
import type { Provider, ProviderStatus } from "../provider.ts";

// Build the ordered list of endpoints to try. The env-resolved one wins; the
// rest cover the common Desktop-free engines so detection "just works".
// Inputs are injectable so the sweep is unit-tested without the real env.
export const candidateSockets = (
  base: Endpoint = resolveEndpoint(),
  home: string = homedir(),
  uid: number | null = typeof process.getuid === "function" ? process.getuid() : null,
  xdg: string | undefined = process.env.XDG_RUNTIME_DIR,
): Endpoint[] => {
  if (base.transport !== "unix") return [base]; // tcp / npipe: trust the env value
  const paths = [
    base.path,
    `${home}/.docker/run/docker.sock`, // Docker Desktop (user socket)
    `${home}/.colima/default/docker.sock`, // Colima
    `${home}/.rd/docker.sock`, // Rancher Desktop
    `${home}/.orbstack/run/docker.sock`, // OrbStack
    xdg ? `${xdg}/docker.sock` : null, // rootless dockerd
    uid !== null ? `/run/user/${uid}/docker.sock` : null,
    uid !== null ? `/run/user/${uid}/podman/podman.sock` : null, // Podman compat
  ].filter((p): p is string => typeof p === "string");
  // De-dup while preserving order.
  const seen = new Set<string>();
  const out: Endpoint[] = [];
  for (const path of paths) {
    if (seen.has(path)) continue;
    seen.add(path);
    out.push({ transport: "unix", path });
  }
  return out;
};

export const existingProvider = (): Provider => ({
  id: "existing",
  label: "External Docker engine",
  managed: false,
  available: async () => true,
  status: async (): Promise<ProviderStatus> => {
    const eps = candidateSockets();
    let best: ProviderStatus | null = null;
    for (const ep of eps) {
      const result = await probe(ep);
      if (result.ok) {
        return { state: "connected", detail: `Connected at ${describeEndpoint(ep)}`, endpoint: ep };
      }
      // Remember the env-resolved (first) endpoint's diagnosis to report.
      if (!best) {
        best = { state: toState(result, { managed: false }), detail: result.detail, endpoint: ep };
      }
    }
    return best ?? { state: "notInstalled", detail: "No Docker engine found", endpoint: null };
  },
});
