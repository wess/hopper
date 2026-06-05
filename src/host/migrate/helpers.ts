// Shared bits for the migration phase modules: the progress emitter, inspect
// projections, and the predefined-network knowledge they all need.

import type { MigrationProgress } from "../../shared/types.ts";

export type Emit = (p: MigrationProgress) => void;

// Emit one progress frame. `extra` carries a non-fatal error or advisory
// warning; absent it's a plain status update.
export const step = (
  emit: Emit,
  phase: MigrationProgress["phase"],
  item: string,
  done: number,
  total: number,
  message: string,
  extra?: { error?: string; warning?: string },
): void =>
  emit({ phase, item, done, total, message, error: extra?.error, warning: extra?.warning });

// Networks that always exist on any daemon — never migrated or recreated, and
// valid as a NetworkMode without any setup on the destination.
export const PREDEFINED_NETWORKS = new Set(["bridge", "host", "none"]);
// NetworkMode values that need no user network on the destination.
export const PREDEFINED_NETMODES = new Set(["", "default", "bridge", "host", "none"]);

// Minimal projections of the inspect responses we read (the engine returns far
// more — we only touch what we recreate).
export type NetworkInspect = {
  Name: string;
  Driver?: string;
  Internal?: boolean;
  Attachable?: boolean;
  EnableIPv6?: boolean;
  Options?: Record<string, string>;
  Labels?: Record<string, string>;
  IPAM?: { Driver?: string; Config?: unknown } | null;
};

export type EndpointSettings = {
  IPAMConfig?: unknown;
  Aliases?: string[] | null;
  Links?: string[] | null;
  MacAddress?: string;
};

export type ContainerInspect = {
  Name?: string;
  Config?: { Image?: string } & Record<string, unknown>;
  HostConfig?: Record<string, unknown>;
  NetworkSettings?: { Networks?: Record<string, EndpointSettings> | null };
};
