import { on } from "butter";

// Direct access to butter's runtime. `idleSeconds` was added to butter's
// public exports after basket pinned its peer dep, so we round-trip through
// the runtime to stay compatible with older butter versions. `listScreens`
// has been a public export for longer, but using the same path here keeps
// the two queries symmetrical.
type ButterRuntime = {
  control: (action: string, data?: unknown) => Promise<unknown>;
};
const getRuntime = (): ButterRuntime => {
  const rt = (globalThis as { __butterRuntime?: ButterRuntime }).__butterRuntime;
  if (!rt) throw new Error("power: butter runtime is not initialized");
  return rt;
};

export type Screen = {
  readonly id: number;
  readonly primary: boolean;
  readonly scale: number;
  readonly bounds: { x: number; y: number; width: number; height: number };
  readonly workArea: { x: number; y: number; width: number; height: number };
};

type Handler = () => void | Promise<void>;

const groups = {
  sleep: [] as Handler[],
  wake: [] as Handler[],
  screenSleep: [] as Handler[],
  screenWake: [] as Handler[],
  lock: [] as Handler[],
  unlock: [] as Handler[],
};

const wired = {
  sleep: false,
  wake: false,
  screenSleep: false,
  screenWake: false,
  lock: false,
  unlock: false,
};

const run = (list: Handler[]): void => {
  void Promise.all(
    list.map((h) =>
      Promise.resolve()
        .then(h)
        .catch(() => {}),
    ),
  );
};

const ensure = (key: keyof typeof wired, event: string, list: Handler[]): void => {
  if (wired[key]) return;
  wired[key] = true;
  on(event, () => run(list));
};

const subscribe = (key: keyof typeof wired, event: string, list: Handler[], handler: Handler): (() => void) => {
  ensure(key, event, list);
  list.push(handler);
  return () => {
    const i = list.indexOf(handler);
    if (i >= 0) list.splice(i, 1);
  };
};

export const onSleep = (handler: Handler) => subscribe("sleep", "power:sleep", groups.sleep, handler);
export const onWake = (handler: Handler) => subscribe("wake", "power:wake", groups.wake, handler);
export const onScreenSleep = (handler: Handler) =>
  subscribe("screenSleep", "power:screensleep", groups.screenSleep, handler);
export const onScreenWake = (handler: Handler) =>
  subscribe("screenWake", "power:screenwake", groups.screenWake, handler);
export const onLock = (handler: Handler) => subscribe("lock", "power:lock", groups.lock, handler);
export const onUnlock = (handler: Handler) => subscribe("unlock", "power:unlock", groups.unlock, handler);

export const idleSeconds = async (): Promise<number> => {
  const r = (await getRuntime().control("power:idle")) as
    | { ok?: boolean; seconds?: number }
    | undefined;
  if (!r || !r.ok || typeof r.seconds !== "number") {
    throw new Error("power: idle query failed (is the shim running?)");
  }
  return r.seconds;
};

export const listScreens = async (): Promise<readonly Screen[]> => {
  const r = (await getRuntime().control("screen:list")) as
    | { ok?: boolean; screens?: Screen[] }
    | undefined;
  if (!r || !r.ok || !Array.isArray(r.screens)) {
    throw new Error("power: screen list query failed (is the shim running?)");
  }
  return r.screens;
};
