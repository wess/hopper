// Where the Docker daemon lives. Resolves a connection target from the
// environment so Hopper works across platforms and against remote daemons:
//
//   • unix socket   — Linux / macOS (the default)
//   • named pipe    — Windows (Docker Desktop's `\\.\pipe\docker_engine`)
//   • tcp           — remote daemons, and the Windows "expose on tcp://" option
//
// Precedence: DOCKER_HOST → DOCKER_SOCKET → per-platform default. Everything
// here is pure (env + os are injectable) so it unit-tests without a daemon.

import { platform } from "node:os";

export type Endpoint =
  | { readonly transport: "unix"; readonly path: string }
  | { readonly transport: "npipe"; readonly path: string }
  | {
      readonly transport: "tcp";
      readonly host: string;
      readonly port: number;
      readonly tls: boolean;
    };

export type DockerEnv = {
  DOCKER_HOST?: string;
  DOCKER_SOCKET?: string;
  DOCKER_TLS_VERIFY?: string;
};

const UNIX_DEFAULT = "/var/run/docker.sock";
const WINDOWS_DEFAULT_PIPE = "//./pipe/docker_engine";

// `npipe:////./pipe/docker_engine` → `\\.\pipe\docker_engine`. Bun/Node name
// pipes with backslashes; we accept the forward-slash URL form and convert.
export const normalizePipe = (p: string): string => p.replace(/\//g, "\\");

const splitHostPort = (authority: string, tls: boolean): { host: string; port: number } => {
  const def = tls ? 2376 : 2375;
  if (authority.startsWith("[")) {
    // IPv6 literal: [::1]:2375
    const end = authority.indexOf("]");
    const host = authority.slice(1, end) || "localhost";
    const rest = authority.slice(end + 1);
    const port = rest.startsWith(":") ? Number(rest.slice(1)) : def;
    return { host, port: Number.isFinite(port) && port > 0 ? port : def };
  }
  const i = authority.lastIndexOf(":");
  if (i === -1) return { host: authority || "localhost", port: def };
  const port = Number(authority.slice(i + 1));
  return {
    host: authority.slice(0, i) || "localhost",
    port: Number.isFinite(port) && port > 0 ? port : def,
  };
};

// Parse a DOCKER_HOST value. Returns null for an unrecognized scheme so the
// caller can fall back rather than connect somewhere wrong.
export const parseDockerHost = (value: string, tlsVerify = false): Endpoint | null => {
  const v = value.trim();
  if (!v) return null;

  if (v.startsWith("unix://")) {
    return { transport: "unix", path: v.slice(7) || UNIX_DEFAULT };
  }
  if (v.startsWith("npipe://")) {
    return { transport: "npipe", path: normalizePipe(v.slice(8)) };
  }
  const m = v.match(/^(tcp|http|https):\/\/(.+)$/);
  if (m) {
    const tls = m[1] === "https" || tlsVerify;
    const { host, port } = splitHostPort(m[2] ?? "", tls);
    return { transport: "tcp", host, port, tls };
  }
  // Bare paths: a unix socket, or a Windows pipe (`//./pipe/…` or `\\.\pipe\…`).
  if (v.startsWith("//") || v.startsWith("\\\\")) {
    return { transport: "npipe", path: normalizePipe(v) };
  }
  if (v.startsWith("/")) return { transport: "unix", path: v };
  return null;
};

const truthy = (s: string | undefined): boolean => s === "1" || s === "true";

// Resolve the active endpoint from the environment + platform.
export const resolveEndpoint = (
  env: DockerEnv = process.env as DockerEnv,
  os: string = platform(),
): Endpoint => {
  if (env.DOCKER_HOST) {
    const ep = parseDockerHost(env.DOCKER_HOST, truthy(env.DOCKER_TLS_VERIFY));
    if (ep) return ep;
  }
  if (env.DOCKER_SOCKET) {
    const s = env.DOCKER_SOCKET;
    return s.startsWith("//") || s.startsWith("\\\\")
      ? { transport: "npipe", path: normalizePipe(s) }
      : { transport: "unix", path: s };
  }
  return os === "win32"
    ? { transport: "npipe", path: normalizePipe(WINDOWS_DEFAULT_PIPE) }
    : { transport: "unix", path: UNIX_DEFAULT };
};

// --- adapters the client / exec layers consume ----------------------------

// Base URL for `fetch`. TCP is addressed directly; socket/pipe transports go
// through a dummy host with the path supplied separately via `unixTarget`.
export const baseUrl = (ep: Endpoint, api: string): string =>
  ep.transport === "tcp"
    ? `${ep.tls ? "https" : "http"}://${ep.host}:${ep.port}/${api}`
    : `http://localhost/${api}`;

// The `unix` option for `fetch` / `Bun.connect` — undefined for TCP. (Bun uses
// the same option for unix sockets and Windows named pipes.)
export const unixTarget = (ep: Endpoint): string | undefined =>
  ep.transport === "tcp" ? undefined : ep.path;

// Options for `Bun.connect` (used by the exec socket hijack).
export const connectOptions = (
  ep: Endpoint,
): { unix: string } | { hostname: string; port: number; tls: boolean } =>
  ep.transport === "tcp" ? { hostname: ep.host, port: ep.port, tls: ep.tls } : { unix: ep.path };

// The HTTP `Host:` header for hand-rolled requests.
export const hostHeader = (ep: Endpoint): string =>
  ep.transport === "tcp" ? `${ep.host}:${ep.port}` : "localhost";

// A human-readable description for the UI / logs.
export const describeEndpoint = (ep: Endpoint): string =>
  ep.transport === "tcp"
    ? `${ep.tls ? "https" : "http"}://${ep.host}:${ep.port}`
    : `${ep.transport}:${ep.path}`;

// A DOCKER_HOST value for child processes (the bundled compose binary, the
// docker CLI) so they target the same engine the client is using.
export const dockerHostValue = (ep: Endpoint): string =>
  ep.transport === "tcp"
    ? `${ep.tls ? "https" : "tcp"}://${ep.host}:${ep.port}`
    : ep.transport === "npipe"
      ? `npipe://${ep.path.replace(/\\/g, "/")}`
      : `unix://${ep.path}`;

// --- the active endpoint ---------------------------------------------------
// Historically the endpoint was frozen into module consts at import time,
// which assumed a daemon was already listening before Hopper started. The
// engine layer needs to point the client at a socket it brings up itself
// (e.g. our VM's forwarded dockerd socket), so the active endpoint is now
// mutable and read per request via `currentEndpoint()`.

let active: Endpoint = resolveEndpoint();

export const currentEndpoint = (): Endpoint => active;

// Override the active endpoint (a provider calls this once it owns a socket).
export const setEndpoint = (ep: Endpoint): void => {
  active = ep;
};

// Re-read the endpoint from the environment + platform. Returns the new value.
export const reresolve = (env?: DockerEnv, os?: string): Endpoint => {
  active = resolveEndpoint(env, os);
  return active;
};
