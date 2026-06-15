// Resolve which compose CLI to drive. Hopper deliberately does not reimplement
// the compose spec (depends_on, healthchecks, profiles, merge semantics …); it
// shells out and streams output. Preference order:
//   1. a bundled standalone `compose` binary (so the feature works on a machine
//      with no docker CLI installed — the point of being a self-contained engine)
//   2. the system `docker compose` v2 plugin
//   3. the legacy hyphenated `docker-compose` v1
// Either way it targets the active engine via DOCKER_HOST.

import { debug } from "../../debug.ts";
import { resolveSidecar } from "../../engine/sidecar.ts";
import { currentEndpoint, dockerHostValue } from "../endpoint.ts";

// `bin` is the executable; `strip` drops the leading "compose" subcommand from
// the arg vector (the standalone binary and v1 take the args directly, the v2
// plugin needs `docker compose …`).
export type Runner = { readonly bin: string; readonly strip: boolean };

let cached: Runner | null = null;

// Child env: point compose at whatever engine the client is currently using.
export const childEnv = (): Record<string, string | undefined> => ({
  ...process.env,
  DOCKER_HOST: dockerHostValue(currentEndpoint()),
});

const probe = async (cmd: string[]): Promise<boolean> => {
  try {
    const proc = Bun.spawn(cmd, { stdout: "ignore", stderr: "ignore", env: childEnv() });
    await proc.exited;
    return proc.exitCode === 0;
  } catch (e) {
    debug("compose", `probe failed for ${cmd[0]}`, e);
    return false;
  }
};

// Resolve the runner once. Cached on the first success; re-probed while none is
// found (so installing docker later is picked up without a restart).
export const resolveRunner = async (): Promise<Runner | null> => {
  if (cached) return cached;
  const bundled = resolveSidecar("compose");
  if (bundled) {
    cached = { bin: bundled, strip: true };
    return cached;
  }
  if (await probe(["docker", "compose", "version"])) {
    cached = { bin: "docker", strip: false };
    return cached;
  }
  if (await probe(["docker-compose", "version"])) {
    cached = { bin: "docker-compose", strip: true };
    return cached;
  }
  return null;
};

// Whether Compose can run at all.
export const available = async (): Promise<boolean> => (await resolveRunner()) !== null;

// Turn an arg vector (always starting with "compose") into a spawnable command
// for the resolved runner.
export const toCommand = (runner: Runner, args: readonly string[]): string[] =>
  runner.strip ? [runner.bin, ...args.slice(1)] : [runner.bin, ...args];
