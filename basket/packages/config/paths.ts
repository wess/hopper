export type AppId = {
  readonly name: string;
  readonly id?: string;
};

export type AppPaths = {
  readonly data: string;
  readonly config: string;
  readonly cache: string;
  readonly logs: string;
};

const home = (): string => {
  const h = Bun.env.HOME ?? Bun.env.USERPROFILE;
  if (!h) throw new Error("Cannot determine home directory: HOME / USERPROFILE not set");
  return h;
};

const join = (...parts: string[]): string => parts.filter(Boolean).join("/").replace(/\/+/g, "/");

const platform = (): "darwin" | "linux" | "win32" => {
  const p = process.platform;
  if (p === "darwin" || p === "linux" || p === "win32") return p;
  return "linux";
};

const xdg = (envName: string, fallback: string): string => Bun.env[envName] ?? fallback;

const macPaths = (name: string): AppPaths => {
  const h = home();
  return {
    data: join(h, "Library", "Application Support", name),
    config: join(h, "Library", "Preferences", name),
    cache: join(h, "Library", "Caches", name),
    logs: join(h, "Library", "Logs", name),
  };
};

const linuxPaths = (name: string): AppPaths => {
  const h = home();
  return {
    data: join(xdg("XDG_DATA_HOME", join(h, ".local", "share")), name),
    config: join(xdg("XDG_CONFIG_HOME", join(h, ".config")), name),
    cache: join(xdg("XDG_CACHE_HOME", join(h, ".cache")), name),
    logs: join(xdg("XDG_STATE_HOME", join(h, ".local", "state")), name, "logs"),
  };
};

const winPaths = (name: string): AppPaths => {
  const h = home();
  const appData = Bun.env.APPDATA ?? join(h, "AppData", "Roaming");
  const localAppData = Bun.env.LOCALAPPDATA ?? join(h, "AppData", "Local");
  return {
    data: join(appData, name),
    config: join(appData, name, "Config"),
    cache: join(localAppData, name, "Cache"),
    logs: join(localAppData, name, "Logs"),
  };
};

export const paths = (app: AppId): AppPaths => {
  const name = app.name;
  switch (platform()) {
    case "darwin":
      return Object.freeze(macPaths(name));
    case "win32":
      return Object.freeze(winPaths(name));
    default:
      return Object.freeze(linuxPaths(name));
  }
};

import { mkdir } from "node:fs/promises";

export const ensurePaths = async (app: AppId): Promise<AppPaths> => {
  const p = paths(app);
  await Promise.all([
    mkdir(p.data, { recursive: true }),
    mkdir(p.config, { recursive: true }),
    mkdir(p.cache, { recursive: true }),
    mkdir(p.logs, { recursive: true }),
  ]);
  return p;
};
