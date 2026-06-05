// Resolve a bundled sidecar binary by name, using Butter's env contract:
//   BUTTER_SIDECARS      — dev:  "name==/abs/path" entries joined by ":::"
//   BUTTER_SIDECARS_DIR  — compiled: "<dir>/<name>" (set by the compiled
//                          bootstrap from the executable's location)
// Returns the absolute path if present on disk, else null.

import { existsSync } from "node:fs";
import { join } from "node:path";

export const resolveSidecar = (name: string): string | null => {
  const raw = process.env.BUTTER_SIDECARS ?? "";
  for (const entry of raw.split(":::")) {
    const eq = entry.indexOf("==");
    if (eq > 0 && entry.slice(0, eq) === name) {
      const path = entry.slice(eq + 2);
      if (existsSync(path)) return path;
    }
  }
  const dir = process.env.BUTTER_SIDECARS_DIR;
  if (dir) {
    const candidate = join(dir, name);
    if (existsSync(candidate)) return candidate;
  }
  return null;
};
