import { on } from "butter";

export type Modifier = "cmd" | "ctrl" | "alt" | "shift";

export type Shortcut = {
  readonly key: string;
  readonly modifiers?: readonly Modifier[];
};

export type ShortcutRegistration = {
  readonly id: string;
  readonly shortcut: Shortcut;
  readonly handler: () => void | Promise<void>;
};

export type ShortcutHandle = {
  readonly id: string;
  readonly unregister: () => void;
};

type Runtime = {
  tell: (action: string, data?: unknown) => void;
};

const runtime = (): Runtime => {
  const r = (globalThis as { __butterRuntime?: Runtime }).__butterRuntime;
  if (!r) throw new Error("Butter runtime not initialized");
  return r;
};

const handlers = new Map<string, () => void | Promise<void>>();
let listening = false;

const ensureListener = (): void => {
  if (listening) return;
  listening = true;
  on("shortcut:triggered", (data) => {
    const id = (data as { id?: string })?.id;
    if (!id) return;
    const h = handlers.get(id);
    if (h) void h();
  });
};

const parse = (spec: string): Shortcut => {
  const parts = spec.split("+").map((s) => s.trim().toLowerCase());
  const mods: Modifier[] = [];
  let key = "";
  for (const part of parts) {
    if (part === "cmd" || part === "command" || part === "meta") mods.push("cmd");
    else if (part === "ctrl" || part === "control") mods.push("ctrl");
    else if (part === "cmdorctrl") mods.push(process.platform === "darwin" ? "cmd" : "ctrl");
    else if (part === "alt" || part === "option") mods.push("alt");
    else if (part === "shift") mods.push("shift");
    else key = part;
  }
  if (!key) throw new Error(`Invalid shortcut: ${spec}`);
  return { key, modifiers: mods };
};

export const registerShortcut = (
  id: string,
  shortcut: Shortcut | string,
  handler: () => void | Promise<void>,
): ShortcutHandle => {
  ensureListener();
  const sc = typeof shortcut === "string" ? parse(shortcut) : shortcut;
  handlers.set(id, handler);
  runtime().tell("shortcut:register", { id, shortcut: sc });
  return {
    id,
    unregister: () => {
      handlers.delete(id);
      runtime().tell("shortcut:unregister", { id });
    },
  };
};

export const unregisterAll = (): void => {
  for (const id of handlers.keys()) {
    runtime().tell("shortcut:unregister", { id });
  }
  handlers.clear();
};

export { parse as parseShortcut };
