// Talk to an arbitrary Docker Engine endpoint. Migration needs two daemons at
// once — the source (e.g. Docker Desktop) and the destination (Hopper's VM) —
// so unlike docker/client.ts (which targets the single active engine) every
// call here takes the endpoint explicitly.

import { baseUrl, type Endpoint, unixTarget } from "../docker/endpoint.ts";

const API = "v1.43";

type Query = Record<string, string | number | boolean | undefined | null>;

const qs = (query?: Query): string => {
  if (!query) return "";
  const parts: string[] = [];
  for (const [k, v] of Object.entries(query)) {
    if (v === undefined || v === null) continue;
    parts.push(`${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`);
  }
  return parts.length ? `?${parts.join("&")}` : "";
};

export type ReqOpts = {
  readonly method?: "GET" | "POST" | "PUT" | "DELETE";
  readonly query?: Query;
  readonly body?: unknown; // JSON value, or a BodyInit when `raw`
  readonly raw?: boolean; // send body verbatim (a tar stream / archive)
  readonly headers?: Record<string, string>;
};

export const reqOn = async (ep: Endpoint, path: string, opts: ReqOpts = {}): Promise<Response> => {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  const init: RequestInit & { unix?: string; duplex?: "half" } = {
    method: opts.method ?? "GET",
    headers,
  };
  const unix = unixTarget(ep);
  if (unix !== undefined) init.unix = unix;
  if (opts.body !== undefined) {
    if (opts.raw) {
      init.body = opts.body as BodyInit;
      if (opts.body instanceof ReadableStream) init.duplex = "half";
    } else {
      init.body = JSON.stringify(opts.body);
      if (!headers["Content-Type"]) headers["Content-Type"] = "application/json";
    }
  }
  const res = await fetch(`${baseUrl(ep, API)}${path}${qs(opts.query)}`, init);
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const data = (await res.json()) as { message?: string };
      if (data?.message) message = data.message;
    } catch {
      // non-JSON error body
    }
    throw new Error(`${path} → ${message}`);
  }
  return res;
};

export const jsonOn = async <T>(ep: Endpoint, path: string, opts: ReqOpts = {}): Promise<T> => {
  const res = await reqOn(ep, path, opts);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
};

// Drain an ndjson stream (e.g. /images/load, /images/create) to completion.
//
// CRITICAL: Docker's streaming endpoints commit HTTP 200 *before* the work runs
// and report failures as in-band frames like {"error":"…"} / {"errorDetail":{…}}.
// reqOn only sees the 200 and won't throw, so we MUST inspect every frame and
// throw on an error — otherwise a failed image load looks like success (silent
// data loss). Mirrors the frame.error convention in docker/images.ts#pull.
export const drainNdjson = async (
  ep: Endpoint,
  path: string,
  opts: ReqOpts,
  onLine?: (obj: Record<string, unknown>) => void,
): Promise<void> => {
  const res = await reqOn(ep, path, opts);
  if (!res.body) return;
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  let streamError: string | undefined;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buf += decoder.decode(value, { stream: true });
      let nl = buf.indexOf("\n");
      while (nl !== -1) {
        const line = buf.slice(0, nl).trim();
        buf = buf.slice(nl + 1);
        if (line) {
          try {
            const obj = JSON.parse(line) as Record<string, unknown>;
            const err =
              typeof obj.error === "string"
                ? obj.error
                : (obj.errorDetail as { message?: string } | undefined)?.message;
            if (err) streamError = err;
            onLine?.(obj);
          } catch {
            // partial / non-JSON frame
          }
        }
        nl = buf.indexOf("\n");
      }
    }
  } finally {
    reader.releaseLock();
  }
  if (streamError) throw new Error(streamError);
};
