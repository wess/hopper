// Sandbox policy for the MCP exec surface.
//
// When Hopper's MCP is exposed over HTTP to another machine (e.g. tandem on the
// LAN), the tools become a remote code-execution surface: `docker.run` and
// `docker.build` let a caller start arbitrary images and run arbitrary
// commands. To keep that bounded, the MCP is meant to point at a THROWAWAY
// docker engine, and the run/build wrappers enforce a hardened HostConfig:
//
//   • AutoRemove           — containers are reaped on exit, no leftover state
//   • CapDrop: ["ALL"]     — drop every Linux capability
//   • SecurityOpt          — no-new-privileges (block setuid escalation)
//   • NetworkMode          — a constrained network ("none" by default)
//   • PidsLimit / mem      — coarse resource caps so a caller can't fork-bomb
//   • NO host bind-mounts  — Binds is always empty; the host fs is never exposed
//
// None of these are negotiable from the tool input — the wrappers overwrite
// whatever the caller asked for. The env only tunes WHICH engine and the
// resource ceilings; it can never relax the isolation flags.
//
// Env (all optional; safe defaults below):
//
//   HOPPER_MCP_DOCKER_HOST   DOCKER_HOST for the isolated engine. When set, the
//                            MCP talks to THIS daemon instead of the desktop
//                            app's engine. Point it at a disposable dockerd
//                            (rootless / nested / a dedicated VM), e.g.
//                            unix:///var/run/sandbox-docker.sock or
//                            tcp://127.0.0.1:2376.
//   HOPPER_MCP_NETWORK       Docker network mode for run/build containers.
//                            Default "none" (no network at all). Set to a
//                            pre-created internal network name to allow egress
//                            you control. "host" is rejected.
//   HOPPER_MCP_PIDS_LIMIT    Max PIDs per container (default 256).
//   HOPPER_MCP_MEMORY_MB     Memory ceiling per container in MiB (default 512).
//   HOPPER_MCP_CPUS          Fractional CPU quota (default 1). 1 = one core.
//   HOPPER_MCP_AUTOREMOVE    "0"/"false" to keep containers after exit (default
//                            on). Disabling defeats the throwaway guarantee —
//                            only do it for debugging the sandbox itself.

export type SandboxConfig = {
  // DOCKER_HOST for the isolated engine, or undefined to use the ambient one.
  readonly dockerHost: string | undefined;
  readonly network: string;
  readonly pidsLimit: number;
  readonly memoryBytes: number;
  // Docker's NanoCPUs unit: 1 CPU == 1e9.
  readonly nanoCpus: number;
  readonly autoRemove: boolean;
};

const num = (v: string | undefined, fallback: number): number => {
  if (v === undefined) return fallback;
  const n = Number(v);
  return Number.isFinite(n) && n > 0 ? n : fallback;
};

const bool = (v: string | undefined, fallback: boolean): boolean => {
  if (v === undefined) return fallback;
  return !(v === "0" || v.toLowerCase() === "false" || v.toLowerCase() === "no");
};

// "host" network would re-expose the host's stack to sandboxed code, so it is
// never honored — fall back to "none".
const network = (v: string | undefined): string => {
  const n = (v ?? "none").trim();
  return n === "" || n.toLowerCase() === "host" ? "none" : n;
};

export const sandboxConfig = (
  env: Record<string, string | undefined> = process.env,
): SandboxConfig => ({
  dockerHost: env.HOPPER_MCP_DOCKER_HOST?.trim() || undefined,
  network: network(env.HOPPER_MCP_NETWORK),
  pidsLimit: Math.floor(num(env.HOPPER_MCP_PIDS_LIMIT, 256)),
  memoryBytes: Math.floor(num(env.HOPPER_MCP_MEMORY_MB, 512) * 1024 * 1024),
  nanoCpus: Math.floor(num(env.HOPPER_MCP_CPUS, 1) * 1_000_000_000),
  autoRemove: bool(env.HOPPER_MCP_AUTOREMOVE, true),
});

// The hardened HostConfig fragment applied to every sandboxed container. The
// caller can never override these — run.ts spreads this LAST over the body.
export const hardenedHostConfig = (cfg: SandboxConfig): Record<string, unknown> => ({
  AutoRemove: cfg.autoRemove,
  // Never bind-mount the host filesystem into a sandbox container.
  Binds: [],
  Mounts: [],
  // Drop every capability and block privilege escalation.
  CapDrop: ["ALL"],
  CapAdd: [],
  SecurityOpt: ["no-new-privileges"],
  Privileged: false,
  ReadonlyRootfs: false,
  // Constrained networking — "none" by default.
  NetworkMode: cfg.network,
  // Coarse resource ceilings.
  PidsLimit: cfg.pidsLimit,
  Memory: cfg.memoryBytes,
  NanoCpus: cfg.nanoCpus,
});
