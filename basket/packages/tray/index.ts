import { on } from "butter";

export type TrayItem = { readonly label: string; readonly action: string } | { readonly separator: true };

export type TrayOptions = {
  readonly title?: string;
  readonly tooltip?: string;
  readonly icon?: string;
  readonly items?: readonly TrayItem[];
};

export type TrayHandle = {
  readonly set: (opts: TrayOptions) => void;
  readonly remove: () => void;
  readonly onAction: (action: string, handler: () => void) => void;
};

type RuntimeShim = {
  tell: (action: string, data?: unknown) => void;
};

const runtime = (): RuntimeShim => {
  const r = (globalThis as { __butterRuntime?: RuntimeShim }).__butterRuntime;
  if (!r) {
    throw new Error("Butter runtime not initialized — ensure your host imports `butter` before creating a tray.");
  }
  return r;
};

const tellTray = (opts: TrayOptions): void => {
  runtime().tell("tray:set", opts);
};

export const createTray = (opts: TrayOptions): TrayHandle => {
  tellTray(opts);
  return {
    set: (next) => tellTray(next),
    remove: () => runtime().tell("tray:remove"),
    onAction: (action, handler) => {
      on(action, () => {
        handler();
      });
    },
  };
};
