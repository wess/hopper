import type { Cache, CacheEntry, CacheOptions } from "./types.ts";

export const memoryCache = (options: CacheOptions = {}): Cache => {
  const map = new Map<string, CacheEntry<unknown>>();
  const defaultTtl = options.defaultTtlMs ?? Number.POSITIVE_INFINITY;

  const isFresh = (entry: CacheEntry<unknown>): boolean => Date.now() < entry.expiresAt;

  return {
    get: async <T>(key: string) => {
      const entry = map.get(key);
      if (!entry || !isFresh(entry)) {
        if (entry) map.delete(key);
        return undefined;
      }
      return entry.value as T;
    },
    set: async (key, value, ttlMs) => {
      const ttl = ttlMs ?? defaultTtl;
      map.set(key, { value, expiresAt: Date.now() + ttl });
    },
    has: async (key) => {
      const entry = map.get(key);
      if (!entry) return false;
      if (!isFresh(entry)) {
        map.delete(key);
        return false;
      }
      return true;
    },
    delete: async (key) => {
      map.delete(key);
    },
    clear: async () => {
      map.clear();
    },
    aside: async <T>(key: string, loader: () => Promise<T> | T, ttlMs?: number) => {
      const entry = map.get(key);
      if (entry && isFresh(entry)) return entry.value as T;
      const value = await loader();
      const ttl = ttlMs ?? defaultTtl;
      map.set(key, { value, expiresAt: Date.now() + ttl });
      return value;
    },
  };
};
