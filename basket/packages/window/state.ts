import type { Store } from "@basket/store";
import { getWindow } from "butter";

export type WindowState = {
  readonly width?: number;
  readonly height?: number;
  readonly x?: number;
  readonly y?: number;
  readonly title?: string;
  readonly fullscreen?: boolean;
  readonly alwaysOnTop?: boolean;
};

const isWindowState = (value: unknown): value is WindowState =>
  typeof value === "object" && value !== null && !Array.isArray(value);

export const saveState = (store: Store, key: string): void => {
  const w = getWindow();
  store.set(key, {
    width: w.width,
    height: w.height,
    x: w.x,
    y: w.y,
    title: w.title,
    fullscreen: w.fullscreen,
    alwaysOnTop: w.alwaysOnTop,
  } satisfies WindowState);
};

export const restoreState = (store: Store, key: string): WindowState | undefined => {
  const raw = store.get(key);
  return isWindowState(raw) ? raw : undefined;
};
