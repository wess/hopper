import { mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { type AppId, paths } from "@basket/config";
import type { Cache, CacheEntry, CacheOptions } from "./types.ts";

export type DiskCacheOptions = CacheOptions & {
  readonly app: AppId;
  readonly file?: string;
};

const readFile = async (path: string): Promise<Record<string, CacheEntry<unknown>>> => {
  const f = Bun.file(path);
  if (!(await f.exists())) return {};
  try {
    return JSON.parse(await f.text()) as Record<string, CacheEntry<unknown>>;
  } catch {
    return {};
  }
};

const writeFile = async (path: string, data: Record<string, CacheEntry<unknown>>): Promise<void> => {
  await mkdir(dirname(path), { recursive: true });
  const tmp = `${path}.${Date.now()}.${Math.random().toString(36).slice(2)}.tmp`;
  await Bun.write(tmp, JSON.stringify(data));
  await Bun.$`mv ${tmp} ${path}`.quiet();
};

export const diskCache = (options: DiskCacheOptions): Cache => {
  const file = join(paths(options.app).cache, options.file ?? "cache.json");
  const defaultTtl = options.defaultTtlMs ?? Number.POSITIVE_INFINITY;

  let state: Record<string, CacheEntry<unknown>> = {};
  let writeQueue: Promise<void> = Promise.resolve();
  const ready = readFile(file).then((loaded) => {
    state = loaded;
  });

  const flush = () => {
    const snap = { ...state };
    writeQueue = writeQueue.then(() => writeFile(file, snap)).catch(() => {});
  };

  const isFresh = (entry: CacheEntry<unknown>): boolean => Date.now() < entry.expiresAt;

  return {
    get: async <T>(key: string) => {
      await ready;
      const entry = state[key];
      if (!entry || !isFresh(entry)) {
        if (entry) {
          const { [key]: _drop, ...rest } = state;
          void _drop;
          state = rest;
          flush();
        }
        return undefined;
      }
      return entry.value as T;
    },
    set: async (key, value, ttlMs) => {
      await ready;
      const ttl = ttlMs ?? defaultTtl;
      state = { ...state, [key]: { value, expiresAt: Date.now() + ttl } };
      flush();
    },
    has: async (key) => {
      await ready;
      const entry = state[key];
      return Boolean(entry && isFresh(entry));
    },
    delete: async (key) => {
      await ready;
      const { [key]: _drop, ...rest } = state;
      void _drop;
      state = rest;
      flush();
    },
    clear: async () => {
      await ready;
      state = {};
      flush();
    },
    aside: async <T>(key: string, loader: () => Promise<T> | T, ttlMs?: number) => {
      await ready;
      const entry = state[key];
      if (entry && isFresh(entry)) return entry.value as T;
      const value = await loader();
      const ttl = ttlMs ?? defaultTtl;
      state = { ...state, [key]: { value, expiresAt: Date.now() + ttl } };
      flush();
      return value;
    },
  };
};
