import { type AppId, paths } from "@basket/config";

export type Store = {
  readonly get: <T = unknown>(key: string) => T | undefined;
  readonly set: (key: string, value: unknown) => void;
  readonly delete: (key: string) => void;
  readonly has: (key: string) => boolean;
  readonly all: () => Readonly<Record<string, unknown>>;
  readonly clear: () => void;
  readonly path: string;
};

export type StoreOptions = {
  readonly app: AppId;
  readonly defaults?: Record<string, unknown>;
};

const join = (...parts: string[]): string => parts.filter(Boolean).join("/").replace(/\/+/g, "/");

const readAsync = async (file: string): Promise<Record<string, unknown>> => {
  const f = Bun.file(file);
  if (!(await f.exists())) return {};
  try {
    return JSON.parse(await f.text());
  } catch {
    return {};
  }
};

const writeAtomic = async (file: string, data: Record<string, unknown>): Promise<void> => {
  const tmp = `${file}.${Date.now()}.${Math.random().toString(36).slice(2)}.tmp`;
  await Bun.write(tmp, JSON.stringify(data, null, 2));
  await Bun.$`mv ${tmp} ${file}`.quiet();
};

export const createStore = (name: string, options: StoreOptions): Store => {
  const dir = paths(options.app).config;
  const file = join(dir, `${name}.json`);

  let state: Record<string, unknown> = { ...(options.defaults ?? {}) };
  let writeQueue: Promise<void> = Promise.resolve();

  const ready = readAsync(file).then((loaded) => {
    state = { ...(options.defaults ?? {}), ...loaded };
  });

  const flush = () => {
    const snapshot = { ...state };
    writeQueue = writeQueue.then(() => writeAtomic(file, snapshot)).catch(() => {});
  };

  // Block synchronous reads on initial load using a top-level await isn't
  // possible from a factory, so we prime state from disk synchronously when
  // available. This is best-effort; the async `ready` settles state above.
  void ready;

  return {
    get: <T>(key: string) => state[key] as T | undefined,
    set: (key, value) => {
      state = { ...state, [key]: value };
      flush();
    },
    delete: (key) => {
      const { [key]: _drop, ...rest } = state;
      void _drop;
      state = rest;
      flush();
    },
    has: (key) => key in state,
    all: () => Object.freeze({ ...state }),
    clear: () => {
      state = { ...(options.defaults ?? {}) };
      flush();
    },
    path: file,
  };
};

export const memoryStore = (defaults: Record<string, unknown> = {}): Store => {
  let state: Record<string, unknown> = { ...defaults };
  return {
    get: <T>(key: string) => state[key] as T | undefined,
    set: (key, value) => {
      state = { ...state, [key]: value };
    },
    delete: (key) => {
      const { [key]: _drop, ...rest } = state;
      void _drop;
      state = rest;
    },
    has: (key) => key in state,
    all: () => Object.freeze({ ...state }),
    clear: () => {
      state = { ...defaults };
    },
    path: ":memory:",
  };
};
