// Hopper's own macOS engine: a Linux VM on Apple's Virtualization.framework
// running dockerd, with its socket forwarded to the host. This is the path
// that lets Hopper *replace* Docker Desktop rather than require it.
//
// The VM itself is driven by a native helper, `hopperd` (see native/hopperd),
// shipped as a bundled sidecar. This module is the host-side provider that
// resolves, supervises, and talks to that helper. Until the helper is bundled
// (and the Butter compiled-path sidecar fix lands) `available()` is false and
// the registry falls back to the external-engine provider, so the app keeps
// working on machines that already have an engine.

import type { EngineState, EngineStats, ReclaimResult } from "../../../../shared/types.ts";
import { type Endpoint, setEndpoint } from "../../../docker/endpoint.ts";
import { backoffDelay, shouldRetry } from "../../backoff.ts";
import { probe } from "../../diagnose.ts";
import type { Provider, ProviderStatus } from "../../provider.ts";
import { resourceEnv } from "../../resources.ts";
import { resolveSidecar } from "../../sidecar.ts";
import { type Control, type HopperdReply, spawnHopperd } from "./control.ts";

const HELPER = "hopperd";
const MAX_RESTARTS = 5;

// The bundled VM helper, resolved via Butter's sidecar env contract.
const resolveHelper = (): string | null => resolveSidecar(HELPER);

// One supervised helper process for the app's lifetime.
let control: Control | null = null;
let userStopped = false;
let restarts = 0;
let restartTimer: ReturnType<typeof setTimeout> | null = null;
// The reason the last start/run failed, so status can explain a dead engine
// instead of the misleading "ready to start".
let lastStartError: string | null = null;

const socketEndpoint = (path: string): Endpoint => ({ transport: "unix", path });

// Crash supervision: if the helper exits and we didn't stop it, respawn with
// capped exponential backoff. Hoisted so `ensureControl` can wire it.
function onHelperExit(): void {
  control = null;
  if (userStopped) return;
  if (!shouldRetry(restarts, MAX_RESTARTS)) {
    lastStartError = `The Hopper VM helper keeps exiting (gave up after ${MAX_RESTARTS} attempts). See ~/.hopper/hopperd.log.`;
    console.warn(`[hopper] ${lastStartError}`);
    return;
  }
  restarts += 1;
  restartTimer = setTimeout(() => {
    void launch();
  }, backoffDelay(restarts));
}

const ensureControl = (): Control | null => {
  if (control?.alive()) return control;
  const bin = resolveHelper();
  if (!bin) return null;
  control = spawnHopperd(bin, resourceEnv());
  control.onExit(onHelperExit);
  return control;
};

// Spawn (if needed) and ask the helper to start the VM. On success, point the
// Docker client at the forwarded socket and reset the restart counter.
const launch = async (): Promise<HopperdReply | null> => {
  const ctl = ensureControl();
  if (!ctl) {
    lastStartError = "The Hopper VM engine is not bundled in this build.";
    return null;
  }
  const reply = await ctl.send("start", 90_000);
  if (reply.state === "running" && reply.socket) {
    restarts = 0;
    lastStartError = null;
    setEndpoint(socketEndpoint(reply.socket));
  } else {
    // Keep the real reason hopperd reported (e.g. "not entitled to use the
    // Virtualization framework", "kernel missing", a VZ error).
    lastStartError = reply.detail || `engine failed to start (${reply.state})`;
  }
  return reply;
};

// Map the helper's reported state to ours. A "running" VM is only reported as
// connected once dockerd actually answers on the forwarded socket — until then
// it's still "starting" (VM up, daemon warming up).
const toEngineState = async (reply: HopperdReply): Promise<ProviderStatus> => {
  if (reply.state === "running" && reply.socket) {
    const endpoint = socketEndpoint(reply.socket);
    const pinged = await probe(endpoint, 1500);
    return pinged.ok
      ? { state: "connected", detail: `Hopper VM at ${reply.socket}`, endpoint }
      : { state: "starting", detail: "VM up, waiting for dockerd…", endpoint: null };
  }
  const map: Record<string, EngineState> = {
    starting: "starting",
    stopped: "stopped",
    error: "unreachable",
  };
  return {
    state: map[reply.state] ?? "stopped",
    detail: reply.detail || reply.state,
    endpoint: null,
  };
};

const vzStatus = async (): Promise<ProviderStatus> => {
  if (process.platform !== "darwin") {
    return {
      state: "unsupported",
      detail: "The Hopper VM engine runs on macOS only",
      endpoint: null,
    };
  }
  if (resolveHelper() === null) {
    return {
      state: "notInstalled",
      detail: "The Hopper VM engine is not bundled in this build yet",
      endpoint: null,
    };
  }
  // Helper not running. If a previous start failed, say why; else it's idle.
  if (!control?.alive()) {
    return lastStartError
      ? { state: "unreachable", detail: lastStartError, endpoint: null }
      : { state: "stopped", detail: "Hopper VM ready to start", endpoint: null };
  }
  return toEngineState(await control.send("status", 4000));
};

const vzStart = async (): Promise<ProviderStatus | undefined> => {
  userStopped = false;
  lastStartError = null;
  if (restartTimer) {
    clearTimeout(restartTimer);
    restartTimer = null;
  }
  const reply = await launch();
  // Surface a failed launch so the caller reports the real reason instead of
  // re-deriving a generic status.
  if (!reply || reply.state !== "running") {
    return {
      state: reply ? "unreachable" : "notInstalled",
      detail: lastStartError ?? "engine failed to start",
      endpoint: null,
    };
  }
};

const vzStop = async (): Promise<void> => {
  userStopped = true;
  if (restartTimer) {
    clearTimeout(restartTimer);
    restartTimer = null;
  }
  restarts = 0;
  if (!control) return;
  await control.shutdown();
  control = null;
};

// Reclaim disk space (fstrim in the guest, which shrinks the host's sparse
// image). No-op unless the VM is running.
const vzReclaim = async (): Promise<ReclaimResult> => {
  if (!control?.alive()) return { ok: false, detail: "engine not running" };
  const reply = await control.send("reclaim", 30_000);
  return { ok: reply.ok, detail: reply.ok ? "Reclaimed unused disk space" : reply.detail };
};

// Live VM stats from the in-guest agent.
const vzStats = async (): Promise<EngineStats | null> => {
  if (!control?.alive()) return null;
  const reply = await control.send("stats", 5_000);
  if (!reply.ok || !reply.data) return null;
  try {
    return JSON.parse(reply.data) as EngineStats;
  } catch {
    return null;
  }
};

export const vzProvider = (): Provider => ({
  id: "vz",
  label: "Hopper VM (Apple Virtualization)",
  managed: true,
  available: async () => process.platform === "darwin" && resolveHelper() !== null,
  status: vzStatus,
  start: vzStart,
  stop: vzStop,
  reclaim: vzReclaim,
  stats: vzStats,
});
