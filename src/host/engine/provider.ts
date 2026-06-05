// What it takes to supply a Docker engine on this machine.
//
// Following Rancher Desktop's shape: one interface, several per-platform
// implementations. A provider answers "can I run here?", reports status, and
// (when it owns the lifecycle) starts/stops the engine. The registry picks one
// and the host drives it; nothing else needs to know whether the engine is a
// native daemon, an external one, or our own VM.

import type { EngineState, EngineStats, ReclaimResult } from "../../shared/types.ts";
import type { Endpoint } from "../docker/endpoint.ts";

export type ProviderStatus = {
  readonly state: EngineState;
  readonly detail: string;
  // Where to talk to this engine, once it's reachable. null when unknown.
  readonly endpoint: Endpoint | null;
};

export type Provider = {
  readonly id: string; // "vz" | "linux" | "existing" | …
  readonly label: string; // human name for settings/UI
  readonly managed: boolean; // true if Hopper owns start/stop
  // Can this provider run on the current machine at all?
  readonly available: () => Promise<boolean>;
  // Current status with no side effects.
  readonly status: () => Promise<ProviderStatus>;
  // Bring the engine up / tear it down — managed providers only. start may
  // return a ProviderStatus so a failed launch can report *why* (otherwise the
  // caller falls back to a fresh status()).
  readonly start?: () => Promise<ProviderStatus | undefined>;
  readonly stop?: () => Promise<void>;
  // Maintenance for managed engines: reclaim disk space, live VM stats.
  readonly reclaim?: () => Promise<ReclaimResult>;
  readonly stats?: () => Promise<EngineStats | null>;
};
