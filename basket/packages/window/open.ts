import type { Store } from "@basket/store";
import { type CreateWindowOptions, closeWindow, createWindow } from "butter";
import { restoreState, type WindowState } from "./state.ts";

export type OpenWindowOptions = {
  readonly url: string;
  readonly defaults?: Omit<CreateWindowOptions, "url">;
  readonly store?: Store;
  readonly storeKey?: string;
};

export type WindowHandle = {
  readonly id: string;
  readonly close: () => void;
};

const merge = (
  defaults: Partial<CreateWindowOptions>,
  restored: WindowState | undefined,
): Partial<CreateWindowOptions> => {
  if (!restored) return defaults;
  return {
    ...defaults,
    ...(restored.width !== undefined ? { width: restored.width } : {}),
    ...(restored.height !== undefined ? { height: restored.height } : {}),
    ...(restored.x !== undefined ? { x: restored.x } : {}),
    ...(restored.y !== undefined ? { y: restored.y } : {}),
    ...(restored.title !== undefined ? { title: restored.title } : {}),
    ...(restored.alwaysOnTop !== undefined ? { alwaysOnTop: restored.alwaysOnTop } : {}),
  };
};

export const openWindow = (options: OpenWindowOptions): WindowHandle => {
  const restored = options.store && options.storeKey ? restoreState(options.store, options.storeKey) : undefined;
  const merged = merge(options.defaults ?? {}, restored);
  const id = createWindow({ url: options.url, ...merged });

  return {
    id,
    close: () => closeWindow(id),
  };
};
