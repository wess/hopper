// Turn "can we reach a Docker engine?" into something the UI can act on.
//
// A bare boolean conflates "no engine installed", "engine stopped", "wrong
// permissions", and "wrong path" — each of which needs a different response.
// `probe` attempts a real `/_ping` against a given endpoint and classifies the
// failure; `toState` maps that into the EngineState the contract exposes.

import { existsSync } from "node:fs";
import type { EngineState } from "../../shared/types.ts";
import { baseUrl, type Endpoint, unixTarget } from "../docker/endpoint.ts";

const API = "v1.43";

// Why a probe failed. `absent` = the socket/pipe path isn't there at all;
// `refused` = path exists but nothing is listening; `denied` = we were not
// allowed to connect; `timeout`/`error` = reached something that misbehaved.
export type ProbeFailure = "absent" | "refused" | "denied" | "timeout" | "error";

export type ProbeResult = { ok: true } | { ok: false; reason: ProbeFailure; detail: string };

// Pull an errno-ish code out of whatever Bun/Node threw.
const codeOf = (err: unknown): string => {
  if (typeof err !== "object" || err === null) return "";
  const e = err as { code?: unknown; name?: unknown; message?: unknown };
  if (typeof e.code === "string") return e.code;
  if (typeof e.name === "string" && e.name) return e.name;
  return typeof e.message === "string" ? e.message : "";
};

// Map a thrown connection error to a failure reason. Pure — unit-tested.
export const classify = (err: unknown): { reason: ProbeFailure; detail: string } => {
  const code = codeOf(err);
  const detail = err instanceof Error ? err.message : String(err);
  if (/ENOENT|not found|no such file/i.test(code)) return { reason: "absent", detail };
  if (/ECONNREFUSED|refused/i.test(code)) return { reason: "refused", detail };
  if (/EACCES|EPERM|denied|permission/i.test(code)) return { reason: "denied", detail };
  if (/ETIMEDOUT|timeout|aborterror|abort/i.test(code)) return { reason: "timeout", detail };
  return { reason: "error", detail };
};

// Attempt a real ping. Socket/pipe paths are stat-checked first so a missing
// path reports `absent` rather than a generic connect error.
export const probe = async (ep: Endpoint, timeoutMs = 2500): Promise<ProbeResult> => {
  if (ep.transport !== "tcp" && !existsSync(ep.path)) {
    return { ok: false, reason: "absent", detail: `No socket at ${ep.path}` };
  }
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), timeoutMs);
  try {
    const init: RequestInit & { unix?: string } = { signal: ac.signal };
    const unix = unixTarget(ep);
    if (unix !== undefined) init.unix = unix;
    const res = await fetch(`${baseUrl(ep, API)}/_ping`, init);
    if (res.ok) return { ok: true };
    return { ok: false, reason: "error", detail: `HTTP ${res.status} ${res.statusText}` };
  } catch (err) {
    return { ok: false, ...classify(err) };
  } finally {
    clearTimeout(timer);
  }
};

// Map a probe result to an EngineState. `managed` providers (ones Hopper can
// start itself) report a missing/refused engine as `stopped` — it can be
// brought up — whereas an unmanaged target with nothing there is `notInstalled`.
export const toState = (result: ProbeResult, opts: { managed: boolean }): EngineState => {
  if (result.ok) return "connected";
  switch (result.reason) {
    case "absent":
      return opts.managed ? "stopped" : "notInstalled";
    case "refused":
      return "stopped";
    case "denied":
      return "needsPermission";
    default:
      return "unreachable";
  }
};
