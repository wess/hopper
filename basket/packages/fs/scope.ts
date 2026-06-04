import { mkdir, readdir, rm, stat } from "node:fs/promises";
import { dirname, isAbsolute, join, normalize, resolve, sep } from "node:path";

export type ScopeEntry = {
  readonly name: string;
  readonly path: string;
  readonly absolute: string;
  readonly isFile: boolean;
  readonly isDirectory: boolean;
  readonly size: number;
};

export type Scope = {
  readonly root: string;
  readonly resolve: (path: string) => string;
  readonly read: (path: string) => Promise<string>;
  readonly readBytes: (path: string) => Promise<Uint8Array>;
  readonly readJson: <T>(path: string) => Promise<T>;
  readonly write: (path: string, content: string | Uint8Array | Blob | ArrayBuffer) => Promise<void>;
  readonly writeJson: (path: string, value: unknown, pretty?: boolean) => Promise<void>;
  readonly exists: (path: string) => Promise<boolean>;
  readonly remove: (path: string) => Promise<void>;
  readonly list: (path?: string) => Promise<readonly ScopeEntry[]>;
  readonly ensureDir: (path?: string) => Promise<void>;
};

const safeResolve = (root: string, target: string): string => {
  if (isAbsolute(target)) {
    const abs = resolve(target);
    if (!abs.startsWith(root + sep) && abs !== root) {
      throw new Error(`Path escapes scope: ${target}`);
    }
    return abs;
  }
  const abs = resolve(root, normalize(target));
  if (!abs.startsWith(root + sep) && abs !== root) {
    throw new Error(`Path escapes scope: ${target}`);
  }
  return abs;
};

export const createScope = (root: string): Scope => {
  const resolvedRoot = resolve(root);

  const r = (p: string) => safeResolve(resolvedRoot, p);

  return {
    root: resolvedRoot,
    resolve: r,

    read: (path) => Bun.file(r(path)).text(),
    readBytes: async (path) => new Uint8Array(await Bun.file(r(path)).arrayBuffer()),
    readJson: async <T>(path: string) => JSON.parse(await Bun.file(r(path)).text()) as T,

    write: async (path, content) => {
      const target = r(path);
      await mkdir(dirname(target), { recursive: true });
      await Bun.write(target, content as Blob);
    },
    writeJson: async (path, value, pretty = true) => {
      const target = r(path);
      await mkdir(dirname(target), { recursive: true });
      await Bun.write(target, JSON.stringify(value, null, pretty ? 2 : 0));
    },

    exists: async (path) => {
      try {
        await stat(r(path));
        return true;
      } catch {
        return false;
      }
    },

    remove: async (path) => {
      await rm(r(path), { recursive: true, force: true });
    },

    list: async (path = "") => {
      const dir = path ? r(path) : resolvedRoot;
      const names = await readdir(dir);
      const out: ScopeEntry[] = [];
      for (const name of names) {
        const abs = join(dir, name);
        const st = await stat(abs).catch(() => undefined);
        if (!st) continue;
        out.push({
          name,
          path: join(path || ".", name),
          absolute: abs,
          isFile: st.isFile(),
          isDirectory: st.isDirectory(),
          size: st.size,
        });
      }
      return out;
    },

    ensureDir: async (path = "") => {
      await mkdir(path ? r(path) : resolvedRoot, { recursive: true });
    },
  };
};
