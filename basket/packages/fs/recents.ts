import type { Store } from "@basket/store";

export type RecentsOptions = {
  readonly store: Store;
  readonly key?: string;
  readonly limit?: number;
};

export type Recents = {
  readonly list: () => readonly string[];
  readonly add: (path: string) => void;
  readonly remove: (path: string) => void;
  readonly clear: () => void;
};

export const createRecents = (options: RecentsOptions): Recents => {
  const key = options.key ?? "recents";
  const limit = options.limit ?? 10;
  const store = options.store;

  const list = (): string[] => {
    const raw = store.get<unknown[]>(key) ?? [];
    return Array.isArray(raw) ? raw.filter((s): s is string => typeof s === "string") : [];
  };

  return {
    list,
    add: (path) => {
      const next = [path, ...list().filter((p) => p !== path)].slice(0, limit);
      store.set(key, next);
    },
    remove: (path) =>
      store.set(
        key,
        list().filter((p) => p !== path),
      ),
    clear: () => store.set(key, []),
  };
};
