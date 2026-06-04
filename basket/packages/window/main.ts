import type { Store } from "@basket/store";
import { setWindow } from "butter";
import { restoreState, saveState, type WindowState } from "./state.ts";

export type MainWindowOptions = {
  readonly defaults: WindowState;
  readonly store?: Store;
  readonly storeKey?: string;
};

export type MainWindowHandle = {
  readonly save: () => void;
  readonly storeKey: string;
};

export const mainWindow = (options: MainWindowOptions): MainWindowHandle => {
  const storeKey = options.storeKey ?? "main";
  const restored = options.store ? restoreState(options.store, storeKey) : undefined;
  const merged: WindowState = { ...options.defaults, ...restored };

  setWindow({
    title: merged.title ?? "App",
    width: merged.width ?? 800,
    height: merged.height ?? 600,
    ...(merged.x !== undefined ? { x: merged.x } : {}),
    ...(merged.y !== undefined ? { y: merged.y } : {}),
    ...(merged.fullscreen !== undefined ? { fullscreen: merged.fullscreen } : {}),
    ...(merged.alwaysOnTop !== undefined ? { alwaysOnTop: merged.alwaysOnTop } : {}),
  });

  return {
    storeKey,
    save: () => {
      if (options.store) saveState(options.store, storeKey);
    },
  };
};
