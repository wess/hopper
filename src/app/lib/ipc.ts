// Webview-side IPC facade. Re-exports the typed `invoke` / `subscribe` from
// @basket/ipc/client plus a couple of React hooks every view leans on.

import type { Channel, Event } from "@basket/ipc";
import { isIpcInvocationError } from "@basket/ipc/client";
import { useEffect, useState } from "react";

export { broadcast, invoke, subscribe } from "@basket/ipc/client";
export { isIpcInvocationError };

import { invoke as rawInvoke, subscribe as rawSubscribe } from "@basket/ipc/client";

export const errorMessage = (e: unknown): string =>
  isIpcInvocationError(e) ? e.message : e instanceof Error ? e.message : String(e);

// Fetch-on-mount + re-fetch helper. `deps` re-runs the load; the returned
// `reload` lets callers refresh imperatively (e.g. after a mutation or a
// `resourcesChanged` event).
export const useLoad = <I, O>(
  channel: Channel<I, O>,
  input: I,
  deps: readonly unknown[] = [],
): { data: O | null; error: string | null; loading: boolean; reload: () => void } => {
  const [data, setData] = useState<O | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: caller-supplied `deps` plus `tick` intentionally drive reloads; channel/input are stable per call site
  useEffect(() => {
    let live = true;
    setLoading(true);
    rawInvoke(channel, input)
      .then((d) => {
        if (live) {
          setData(d);
          setError(null);
        }
      })
      .catch((e) => {
        if (live) setError(errorMessage(e));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [tick, ...deps]);

  return { data, error, loading, reload: () => setTick((t) => t + 1) };
};

// Subscribe to a host event for the lifetime of a component.
export const useEvent = <T>(event: Event<T>, fn: (data: T) => void): void => {
  useEffect(() => rawSubscribe(event, fn), [event, fn]);
};
