// Pick and drive the right engine provider for this machine.
//
// Preference order per platform; the first available provider wins and
// `existing` is always the tail fallback so Hopper keeps working against an
// engine someone else runs. The host calls `engineStatus()` each poll and
// `startEngine()`/`stopEngine()` from the UI; this module also points the
// Docker client at the chosen endpoint once it's reachable.

import { platform } from "node:os";
import type { EngineState, EngineStats, EngineStatus, ReclaimResult } from "../../shared/types.ts";
import { describeEndpoint, setEndpoint } from "../docker/endpoint.ts";
import type { Provider } from "./provider.ts";
import { existingProvider } from "./providers/existing.ts";
import { linuxProvider } from "./providers/linux.ts";
import { vzProvider } from "./providers/vz/index.ts";

export const candidatesFor = (os: string): Provider[] => {
  if (os === "darwin") return [vzProvider(), existingProvider()];
  if (os === "linux") return [linuxProvider(), existingProvider()];
  return [existingProvider()]; // win32: managed WSL2 provider lands later
};

// An explicit user choice (e.g. "always use my external engine"), or null to
// auto-pick. Seeded from the environment for power users / tests.
let preference: string | null = process.env.HOPPER_ENGINE ?? null;

export const setPreferredProvider = (id: string | null): void => {
  preference = id;
};

// Pick from an ordered candidate list: an available preferred provider wins,
// else the first available one, else the last candidate (always `existing`,
// which is always available). Pure given the candidates — unit-tested.
export const selectProvider = async (
  candidates: Provider[],
  pref: string | null,
): Promise<Provider> => {
  if (pref) {
    const chosen = candidates.find((c) => c.id === pref);
    if (chosen && (await chosen.available())) return chosen;
  }
  for (const candidate of candidates) {
    if (await candidate.available()) return candidate;
  }
  return candidates[candidates.length - 1] ?? existingProvider();
};

// Re-evaluated each call (the checks are cheap) so a provider that becomes
// available — our VM engine once bundled — is picked up without a restart.
export const activeProvider = async (): Promise<Provider> =>
  selectProvider(candidatesFor(platform()), preference);

export const messageFor = (state: EngineState, provider: Provider): string => {
  switch (state) {
    case "connected":
      return "Connected to Docker Engine";
    case "starting":
      return `Starting ${provider.label}…`;
    case "stopped":
      return provider.managed ? `${provider.label} is stopped` : "Docker engine is not running";
    case "notInstalled":
      return "No Docker engine found";
    case "unreachable":
      return "Cannot reach the Docker engine";
    case "needsPermission":
      return "Permission denied connecting to the Docker engine";
    default:
      return "No engine provider available for this platform";
  }
};

const report = (
  provider: Provider,
  state: EngineState,
  detail: string,
  endpoint?: string,
): EngineStatus => ({
  state,
  connected: state === "connected",
  message: messageFor(state, provider),
  provider: provider.id,
  managed: provider.managed,
  detail,
  endpoint,
});

export const engineStatus = async (): Promise<EngineStatus> => {
  const provider = await activeProvider();
  const status = await provider.status();
  // Point the client at the live endpoint so every request reaches it.
  if (status.state === "connected" && status.endpoint) setEndpoint(status.endpoint);
  return report(
    provider,
    status.state,
    status.detail,
    status.endpoint ? describeEndpoint(status.endpoint) : undefined,
  );
};

export const startEngine = async (): Promise<EngineStatus> => {
  const provider = await activeProvider();
  if (provider.start) {
    // If the launch failed, report exactly why rather than re-deriving a
    // generic status from a fresh probe a moment later.
    const result = await provider.start();
    if (result && result.state !== "connected") {
      return report(
        provider,
        result.state,
        result.detail,
        result.endpoint ? describeEndpoint(result.endpoint) : undefined,
      );
    }
  }
  return engineStatus();
};

export const stopEngine = async (): Promise<EngineStatus> => {
  const provider = await activeProvider();
  if (provider.stop) await provider.stop();
  return engineStatus();
};

export const reclaimEngine = async (): Promise<ReclaimResult> => {
  const provider = await activeProvider();
  return provider.reclaim
    ? provider.reclaim()
    : { ok: false, detail: "Not supported by the active engine" };
};

export const engineStatsNow = async (): Promise<EngineStats | null> => {
  const provider = await activeProvider();
  return provider.stats ? provider.stats() : null;
};
