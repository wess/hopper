// Compose-stack helpers for the Containers view. A "stack" is the set of
// containers sharing a `composeProject` label; these derive its aggregate
// state and the lifecycle ops that make sense for it.

import type { Container } from "../../shared/types.ts";

export type StackState = "running" | "partial" | "stopped";

// Aggregate a stack's state: running if every member runs, stopped if none do,
// partial in between.
export const stackState = (
  items: readonly Container[],
): { running: number; total: number; state: StackState } => {
  const total = items.length;
  const running = items.filter((c) => c.state === "running").length;
  const state: StackState = running === 0 ? "stopped" : running === total ? "running" : "partial";
  return { running, total, state };
};
