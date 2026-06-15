// Run a compose lifecycle action and stream its output. Subscribes to
// composeProgress filtered by a generated requestId, forwards each line to
// `onLine`, and resolves with the final ok/error when the action completes.

import * as ch from "../../shared/channels.ts";
import type {
  ComposeAction,
  ComposeOptions,
  ComposeProgress,
  ComposeTarget,
} from "../../shared/types.ts";
import { invoke, subscribe } from "./ipc.ts";

export const runComposeAction = async (
  action: ComposeAction,
  target: ComposeTarget,
  options: ComposeOptions | undefined,
  onLine: (line: string, stream: "stdout" | "stderr") => void,
): Promise<{ ok: boolean; error?: string }> => {
  const requestId = crypto.randomUUID();
  const unsub = subscribe(ch.composeProgress, (p: ComposeProgress) => {
    if (p.requestId !== requestId) return;
    if (p.line) onLine(p.line, p.stream);
  });
  try {
    return await invoke(ch.composeAction, { requestId, action, target, options });
  } finally {
    unsub();
  }
};
