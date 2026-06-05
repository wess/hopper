// Configurable VM resources (CPU / memory / disk) for the managed engine,
// persisted to ~/.hopper/resources.json and passed to hopperd via env when it
// spawns. CPU + memory apply on (re)start; disk size applies to a freshly
// created data disk. Values are clamped to sane bounds.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, cpus as hostCpus } from "node:os";
import { dirname, join } from "node:path";
import type { EngineResources } from "../../shared/types.ts";

const cores = (): number => {
  try {
    return hostCpus().length || 4;
  } catch {
    return 4;
  }
};

export const defaultResources = (): EngineResources => ({
  cpus: Math.max(2, Math.floor(cores() / 2)),
  memoryGiB: 4,
  diskGiB: 60,
});

const file = (): string =>
  join(process.env.HOPPER_HOME ?? join(homedir(), ".hopper"), "resources.json");

// Clamp partial input to sane bounds, filling gaps from defaults. Pure —
// unit-tested.
export const clampResources = (input: Partial<EngineResources>): EngineResources => {
  const d = defaultResources();
  const int = (v: number | undefined, fallback: number) =>
    Number.isFinite(v) ? Math.floor(v as number) : fallback;
  return {
    cpus: Math.max(1, Math.min(int(input.cpus, d.cpus), cores())),
    memoryGiB: Math.max(1, Math.min(int(input.memoryGiB, d.memoryGiB), 64)),
    diskGiB: Math.max(8, Math.min(int(input.diskGiB, d.diskGiB), 1024)),
  };
};

let current: EngineResources | null = null;

export const getResources = (): EngineResources => {
  if (current) return current;
  try {
    current = clampResources(JSON.parse(readFileSync(file(), "utf8")));
  } catch {
    current = defaultResources();
  }
  return current;
};

export const setResources = (input: Partial<EngineResources>): EngineResources => {
  current = clampResources(input);
  try {
    mkdirSync(dirname(file()), { recursive: true });
    writeFileSync(file(), JSON.stringify(current, null, 2));
  } catch {
    // best effort — the env still reflects `current` for this session
  }
  return current;
};

// The env hopperd reads (see native/hopperd/config.swift).
export const resourceEnv = (): Record<string, string> => {
  const r = getResources();
  return {
    HOPPER_CPUS: String(r.cpus),
    HOPPER_MEMORY_GIB: String(r.memoryGiB),
    HOPPER_DISK_GIB: String(r.diskGiB),
  };
};
