export type CacheEntry<T> = {
  readonly value: T;
  readonly expiresAt: number;
};

export type Cache = {
  readonly get: <T>(key: string) => Promise<T | undefined>;
  readonly set: <T>(key: string, value: T, ttlMs?: number) => Promise<void>;
  readonly has: (key: string) => Promise<boolean>;
  readonly delete: (key: string) => Promise<void>;
  readonly clear: () => Promise<void>;
  readonly aside: <T>(key: string, loader: () => Promise<T> | T, ttlMs?: number) => Promise<T>;
};

export type CacheOptions = {
  readonly defaultTtlMs?: number;
};
