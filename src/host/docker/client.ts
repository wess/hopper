// Low-level Docker Engine API client. Talks HTTP to the daemon over whichever
// transport `endpoint.ts` resolves — a unix socket (Linux/macOS), a Windows
// named pipe, or TCP (remote / Windows "expose on tcp://"). Every higher-level
// module (containers, images, …) is built on `req` / `json` / `stream` here.

import { baseUrl, resolveEndpoint, unixTarget } from "./endpoint.ts";

// Pinned to a version inside the daemon's supported window [1.40, 1.54].
const API = "v1.43";
const ENDPOINT = resolveEndpoint();
const BASE = baseUrl(ENDPOINT, API);
// Socket/pipe path for `fetch`'s `unix` option; undefined when using TCP.
const UNIX = unixTarget(ENDPOINT);

export type Query = Record<string, string | number | boolean | undefined | null>;

const qs = (query?: Query): string => {
  if (!query) return "";
  const parts: string[] = [];
  for (const [k, v] of Object.entries(query)) {
    if (v === undefined || v === null) continue;
    parts.push(`${encodeURIComponent(k)}=${encodeURIComponent(String(v))}`);
  }
  return parts.length ? `?${parts.join("&")}` : "";
};

export type ReqOptions = {
  readonly method?: "GET" | "POST" | "PUT" | "DELETE" | "HEAD";
  readonly query?: Query;
  readonly body?: unknown;
  // Extra request headers. Used for `X-Registry-Auth` (push) and to override
  // the content type when sending a non-JSON body (e.g. a build context tar).
  readonly headers?: Readonly<Record<string, string>>;
  readonly signal?: AbortSignal;
};

// A body that must be sent verbatim rather than JSON-encoded — binary or
// streaming payloads such as an image build context tarball.
export const isRawBody = (b: unknown): b is BodyInit =>
  b instanceof Uint8Array ||
  b instanceof ArrayBuffer ||
  b instanceof Blob ||
  b instanceof ReadableStream;

// Raw request — returns the Response so callers can read JSON, text, or a
// streaming body. Throws a `DockerError` for non-2xx with the daemon message.
export const req = async (path: string, opts: ReqOptions = {}): Promise<Response> => {
  const headers: Record<string, string> = { ...(opts.headers ?? {}) };
  const init: RequestInit & { unix?: string; duplex?: "half" } = {
    method: opts.method ?? "GET",
    signal: opts.signal,
    headers,
  };
  // TCP endpoints are addressed by the BASE url; socket/pipe ones need `unix`.
  if (UNIX !== undefined) init.unix = UNIX;
  if (opts.body !== undefined) {
    if (isRawBody(opts.body)) {
      init.body = opts.body;
      // A streaming request body requires half-duplex mode under fetch.
      if (opts.body instanceof ReadableStream) init.duplex = "half";
    } else {
      init.body = JSON.stringify(opts.body);
      if (!headers["Content-Type"]) headers["Content-Type"] = "application/json";
    }
  }
  const res = await fetch(`${BASE}${path}${qs(opts.query)}`, init);
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      const data = (await res.json()) as { message?: string };
      if (data?.message) message = data.message;
    } catch {
      // non-JSON error body — keep the status line
    }
    throw new DockerError(message, res.status);
  }
  return res;
};

export class DockerError extends Error {
  readonly status: number;
  constructor(message: string, status: number) {
    super(message);
    this.name = "DockerError";
    this.status = status;
  }
}

// JSON request — the common case. `T` is the expected decoded shape.
export const json = async <T>(path: string, opts: ReqOptions = {}): Promise<T> => {
  const res = await req(path, opts);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
};

// POST/DELETE with no meaningful body (start, stop, remove, …).
export const action = async (path: string, opts: ReqOptions = {}): Promise<void> => {
  await req(path, { method: "POST", ...opts });
};

// Stream a newline-delimited JSON body (events, stats, pull progress).
// Yields one parsed object per line until the stream ends or aborts.
export async function* ndjson<T>(path: string, opts: ReqOptions = {}): AsyncGenerator<T> {
  const res = await req(path, opts);
  if (!res.body) return;
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let nl = buffer.indexOf("\n");
      while (nl !== -1) {
        const line = buffer.slice(0, nl).trim();
        buffer = buffer.slice(nl + 1);
        if (line) {
          try {
            yield JSON.parse(line) as T;
          } catch {
            // partial / non-JSON frame — skip
          }
        }
        nl = buffer.indexOf("\n");
      }
    }
  } finally {
    reader.releaseLock();
  }
}

// Raw byte stream — for container logs, which use Docker's stdcopy multiplex
// framing (unless the container has a TTY). `onChunk` receives demuxed UTF-8
// text. Resolves when the stream ends; abort via `signal`.
export const streamLogs = async (
  path: string,
  opts: ReqOptions,
  onChunk: (text: string, stream: "stdout" | "stderr") => void,
): Promise<void> => {
  const res = await req(path, opts);
  if (!res.body) return;
  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  // Docker multiplexes stdout/stderr with an 8-byte header per frame:
  // [STREAM_TYPE, 0,0,0, SIZE(uint32 BE)]. We buffer raw bytes and peel
  // frames; if the bytes don't look like framed output (TTY mode) we fall
  // back to emitting them as plain stdout.
  let buf = new Uint8Array(0);
  const append = (chunk: Uint8Array) => {
    const next = new Uint8Array(buf.length + chunk.length);
    next.set(buf);
    next.set(chunk, buf.length);
    buf = next;
  };
  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      if (!value) continue;
      append(value);
      // Peel as many complete frames as we have.
      while (buf.length >= 8) {
        const type = buf[0];
        // A valid header has type 0/1/2 and zero pad bytes.
        const framed =
          (type === 0 || type === 1 || type === 2) && buf[1] === 0 && buf[2] === 0 && buf[3] === 0;
        if (!framed) {
          // TTY / unframed — flush everything as stdout.
          onChunk(decoder.decode(buf, { stream: true }), "stdout");
          buf = new Uint8Array(0);
          break;
        }
        const size = (buf[4]! << 24) | (buf[5]! << 16) | (buf[6]! << 8) | buf[7]!;
        if (buf.length < 8 + size) break; // wait for the rest of the frame
        const payload = buf.slice(8, 8 + size);
        buf = buf.slice(8 + size);
        onChunk(decoder.decode(payload, { stream: true }), type === 2 ? "stderr" : "stdout");
      }
    }
  } finally {
    reader.releaseLock();
  }
};
