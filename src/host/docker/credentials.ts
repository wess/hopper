// Registry credentials, resolved from the user's existing Docker config
// (`~/.docker/config.json` + credential helpers). Hopper never stores secrets
// of its own — it reuses whatever `docker login` already set up, so push
// inherits the same auth the CLI uses.

import { homedir } from "node:os";
import { join } from "node:path";

export type DockerConfig = {
  auths?: Record<string, { auth?: string; identitytoken?: string }>;
  credsStore?: string;
  credHelpers?: Record<string, string>;
};

// What the daemon wants in the base64 `X-Registry-Auth` header.
export type RegistryAuth = {
  username?: string;
  password?: string;
  identitytoken?: string;
  serveraddress: string;
};

// Docker Hub credentials live under this canonical key, not under "docker.io".
const DOCKER_HUB_SERVER = "https://index.docker.io/v1/";

// Strip scheme + trailing slash so `https://host/` and `host` compare equal.
const bare = (s: string): string => s.replace(/^https?:\/\//, "").replace(/\/+$/, "");

// Derive the registry server address from an image reference. Bare names and
// `docker.io/...` map to Hub's canonical endpoint; otherwise the first path
// segment is a registry host when it has a dot, a port colon, or is localhost.
export const registryHost = (ref: string): string => {
  const slash = ref.indexOf("/");
  if (slash === -1) return DOCKER_HUB_SERVER;
  const head = ref.slice(0, slash);
  if (head === "docker.io") return DOCKER_HUB_SERVER;
  if (head.includes(".") || head.includes(":") || head === "localhost") return head;
  return DOCKER_HUB_SERVER;
};

export const parseDockerConfig = (text: string): DockerConfig => {
  try {
    return JSON.parse(text) as DockerConfig;
  } catch {
    return {};
  }
};

// Base64-encode an auth object for the `X-Registry-Auth` header.
export const encodeRegistryAuth = (auth: RegistryAuth): string =>
  Buffer.from(JSON.stringify(auth)).toString("base64");

// Decode a docker config `auth` entry ("base64(user:pass)").
export const decodeAuthEntry = (b64: string): { username?: string; password?: string } => {
  try {
    const decoded = Buffer.from(b64, "base64").toString("utf8");
    const i = decoded.indexOf(":");
    if (i === -1) return {};
    return { username: decoded.slice(0, i), password: decoded.slice(i + 1) };
  } catch {
    return {};
  }
};

// Find the config key (auths/credHelpers) that matches a server, comparing
// scheme/slash-insensitively.
export const matchKey = (keys: readonly string[], server: string): string | undefined => {
  if (keys.includes(server)) return server;
  const want = bare(server);
  return keys.find((k) => bare(k) === want);
};

const configPath = (): string => {
  const dir = process.env.DOCKER_CONFIG;
  return dir ? join(dir, "config.json") : join(homedir(), ".docker", "config.json");
};

const readConfig = async (): Promise<DockerConfig> => {
  const file = Bun.file(configPath());
  if (!(await file.exists())) return {};
  return parseDockerConfig(await file.text());
};

// Ask a credential helper (`docker-credential-<helper> get`) for a server's
// secret. Returns null if the helper is missing or has no entry.
const credHelperGet = async (helper: string, server: string): Promise<RegistryAuth | null> => {
  try {
    const proc = Bun.spawn([`docker-credential-${helper}`, "get"], {
      stdin: "pipe",
      stdout: "pipe",
      stderr: "ignore",
    });
    proc.stdin.write(server);
    await proc.stdin.end();
    const out = await new Response(proc.stdout).text();
    await proc.exited;
    if (proc.exitCode !== 0) return null;
    const c = JSON.parse(out) as { Username?: string; Secret?: string };
    if (!c.Username && !c.Secret) return null;
    // Helpers signal token auth by returning the sentinel username "<token>".
    if (c.Username === "<token>") return { identitytoken: c.Secret, serveraddress: server };
    return { username: c.Username, password: c.Secret, serveraddress: server };
  } catch {
    return null;
  }
};

// Resolve push/pull credentials for an image ref, preferring a credential
// helper (the secure path) over a plaintext `auths` entry. Returns null when
// no credentials are configured (anonymous).
export const resolveAuth = async (ref: string): Promise<RegistryAuth | null> => {
  const server = registryHost(ref);
  const cfg = await readConfig();

  const helpers = cfg.credHelpers ?? {};
  const helperKey = matchKey(Object.keys(helpers), server);
  const helper = (helperKey && helpers[helperKey]) || cfg.credsStore;
  if (helper) {
    const fromHelper = await credHelperGet(helper, server);
    if (fromHelper) return fromHelper;
  }

  const auths = cfg.auths ?? {};
  const authKey = matchKey(Object.keys(auths), server);
  if (authKey) {
    const entry = auths[authKey] ?? {};
    if (entry.identitytoken) return { identitytoken: entry.identitytoken, serveraddress: server };
    if (entry.auth) return { ...decodeAuthEntry(entry.auth), serveraddress: server };
  }
  return null;
};

// Logged-in registry hosts for the Settings panel — names only, no secrets.
export const listRegistries = async (): Promise<string[]> => {
  const cfg = await readConfig();
  const set = new Set<string>([
    ...Object.keys(cfg.auths ?? {}),
    ...Object.keys(cfg.credHelpers ?? {}),
  ]);
  return [...set].sort();
};
