// Native dockerd on Linux — the trivial case. The engine runs on the host
// kernel, so there is no VM: detect the right socket (rootful or rootless),
// surface a permission problem clearly (the classic "add yourself to the
// docker group"), and best-effort start a user/rootless daemon via systemd.

import { describeEndpoint, type Endpoint, resolveEndpoint } from "../../docker/endpoint.ts";
import { probe, toState } from "../diagnose.ts";
import type { Provider, ProviderStatus } from "../provider.ts";

const sockets = (): Endpoint[] => {
  const env = resolveEndpoint();
  const uid = typeof process.getuid === "function" ? process.getuid() : null;
  const xdg = process.env.XDG_RUNTIME_DIR;
  const paths = [
    env.transport === "unix" ? env.path : null,
    "/var/run/docker.sock", // rootful
    xdg ? `${xdg}/docker.sock` : null, // rootless
    uid !== null ? `/run/user/${uid}/docker.sock` : null,
  ].filter((p): p is string => typeof p === "string");
  const seen = new Set<string>();
  const out: Endpoint[] = [];
  for (const path of paths) {
    if (seen.has(path)) continue;
    seen.add(path);
    out.push({ transport: "unix", path });
  }
  return out;
};

const run = async (cmd: string[]): Promise<boolean> => {
  try {
    const proc = Bun.spawn(cmd, { stdout: "ignore", stderr: "ignore" });
    return (await proc.exited) === 0;
  } catch {
    return false;
  }
};

export const linuxProvider = (): Provider => ({
  id: "linux",
  label: "Native Docker Engine (Linux)",
  managed: true,
  available: async () => process.platform === "linux",
  status: async (): Promise<ProviderStatus> => {
    let best: ProviderStatus | null = null;
    for (const ep of sockets()) {
      const result = await probe(ep);
      if (result.ok) {
        return { state: "connected", detail: `Connected at ${describeEndpoint(ep)}`, endpoint: ep };
      }
      if (!best) {
        const state = toState(result, { managed: true });
        const where = ep.transport === "unix" ? ep.path : describeEndpoint(ep);
        const detail =
          state === "needsPermission"
            ? `Permission denied on ${where}. Add your user to the "docker" group (note: root-equivalent), or run rootless.`
            : result.detail;
        best = { state, detail, endpoint: ep };
      }
    }
    return best ?? { state: "stopped", detail: "dockerd not running", endpoint: null };
  },
  // Try the rootless user daemon first (no privileges), then the system unit.
  start: async (): Promise<void> => {
    if (await run(["systemctl", "--user", "start", "docker"])) return;
    await run(["systemctl", "start", "docker"]);
  },
  stop: async (): Promise<void> => {
    if (await run(["systemctl", "--user", "stop", "docker"])) return;
    await run(["systemctl", "stop", "docker"]);
  },
});
