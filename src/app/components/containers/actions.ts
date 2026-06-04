// Container lifecycle action helpers — each invokes a channel and toasts the
// outcome. Shared by row action buttons, the detail header, and bulk actions.

import type { Channel } from "@basket/ipc";
import { toast } from "@basket/ui/toast";
import * as ch from "../../../shared/channels.ts";
import type { Container } from "../../../shared/types.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";

const run = async (
  channel: Channel<{ id: string }, void>,
  id: string,
  verb: string,
): Promise<void> => {
  try {
    await invoke(channel, { id });
    toast.success(`Container ${verb}`);
  } catch (e) {
    toast.error(`Failed to ${verb.replace(/ed$/, "")}`, { description: errorMessage(e) });
  }
};

export const start = (id: string) => run(ch.containerStart, id, "started");
export const stop = (id: string) => run(ch.containerStop, id, "stopped");
export const restart = (id: string) => run(ch.containerRestart, id, "restarted");
export const pause = (id: string) => run(ch.containerPause, id, "paused");
export const unpause = (id: string) => run(ch.containerUnpause, id, "resumed");
export const kill = (id: string) => run(ch.containerKill, id, "killed");

export const remove = async (c: Container): Promise<boolean> => {
  const force = c.state === "running" || c.state === "paused" || c.state === "restarting";
  try {
    await invoke(ch.containerRemove, { id: c.id, force });
    toast.success(`Removed ${c.name}`);
    return true;
  } catch (e) {
    toast.error("Failed to remove", { description: errorMessage(e) });
    return false;
  }
};

export const batch = async (
  ids: readonly string[],
  op: "start" | "stop" | "restart" | "remove",
): Promise<void> => {
  try {
    await invoke(ch.containerBatch, { ids: [...ids], op });
    toast.success(`${op} ${ids.length} container${ids.length === 1 ? "" : "s"}`);
  } catch (e) {
    toast.error(`Batch ${op} failed`, { description: errorMessage(e) });
  }
};
