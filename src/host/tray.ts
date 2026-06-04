// macOS menubar (status-bar) app for Hopper. Shows a 🐳 item with live engine
// and container status, quick navigation into the app, and a quit action.
// Polls the Docker engine every few seconds and rebuilds the menu in place.
import { emit } from "@basket/ipc";
import { createTray, type TrayItem } from "@basket/tray";
import { type MainWindowHandle, restore, setAlwaysOnTop } from "@basket/window";
import * as ch from "../shared/channels.ts";
import * as containers from "./docker/containers.ts";
import * as system from "./docker/system.ts";

const POLL_MS = 4_000;
const MAX_LISTED = 6;

type TrayState = {
  readonly online: boolean;
  readonly running: number;
  readonly total: number;
  readonly names: readonly string[];
};

// Best-effort: bring the main window forward without throwing if it no-ops.
const focusWindow = (): void => {
  try {
    // Un-minimize/raise, then briefly assert always-on-top to pull it forward.
    restore();
    setAlwaysOnTop(true);
    setAlwaysOnTop(false);
  } catch {
    // window may not be present; ignore.
  }
};

const title = (state: TrayState): string =>
  state.online && state.running > 0 ? `🐳 ${state.running}` : "🐳";

const tooltip = (state: TrayState): string =>
  state.online
    ? `Hopper — ${state.running} running, ${state.total} total`
    : "Hopper — engine offline";

// Pure: derive the full menu from the current state snapshot.
const buildItems = (state: TrayState): TrayItem[] => {
  const items: TrayItem[] = [];

  items.push({
    label: state.online ? "● Engine running" : "○ Engine offline",
    action: "tray:status",
  });
  if (state.online) {
    items.push({ label: `${state.running} running, ${state.total} total`, action: "tray:status" });
  }

  items.push({ separator: true });
  items.push({ label: "Open Hopper", action: "tray:open" });
  items.push({ label: "Dashboard", action: "tray:nav:dashboard" });
  items.push({ label: "Containers", action: "tray:nav:containers" });
  items.push({ label: "Images", action: "tray:nav:images" });

  items.push({ separator: true });
  if (state.names.length > 0) {
    for (const name of state.names.slice(0, MAX_LISTED)) {
      items.push({ label: `  ${name}`, action: "tray:open" });
    }
  } else {
    items.push({ label: "No running containers", action: "tray:status" });
  }

  items.push({ separator: true });
  items.push({ label: "Quit Hopper", action: "tray:quit" });

  return items;
};

// Probe the engine for a fresh snapshot. Never throws; offline on any failure.
const snapshot = async (): Promise<TrayState> => {
  const offline: TrayState = { online: false, running: 0, total: 0, names: [] };
  try {
    const online = await system.ping();
    if (!online) return offline;

    let running = 0;
    let total = 0;
    try {
      const info = await system.info();
      running = info.containersRunning;
      total = info.containers;
    } catch {
      // info can fail mid-startup; fall through to the list-based counts.
    }

    let names: string[] = [];
    try {
      const all = await containers.list(true);
      const live = all.filter((c) => c.state === "running");
      names = live.map((c) => c.name).filter(Boolean);
      if (total <= 0) total = all.length;
      if (running <= 0) running = live.length;
    } catch {
      // listing may fail transiently; keep whatever info gave us.
    }

    return { online: true, running, total, names };
  } catch {
    return offline;
  }
};

export const startTray = (win: MainWindowHandle): void => {
  let state: TrayState = { online: false, running: 0, total: 0, names: [] };

  const tray = createTray({
    title: title(state),
    tooltip: tooltip(state),
    items: buildItems(state),
  });

  // Register action handlers once; ticks only call set() to refresh the menu.
  tray.onAction("tray:status", () => {});
  tray.onAction("tray:open", focusWindow);
  tray.onAction("tray:nav:dashboard", () => {
    focusWindow();
    emit(ch.trayNavigate, "dashboard");
  });
  tray.onAction("tray:nav:containers", () => {
    focusWindow();
    emit(ch.trayNavigate, "containers");
  });
  tray.onAction("tray:nav:images", () => {
    focusWindow();
    emit(ch.trayNavigate, "images");
  });
  tray.onAction("tray:quit", () => {
    win.save();
    process.exit(0);
  });

  const tick = async (): Promise<void> => {
    try {
      state = await snapshot();
      tray.set({ title: title(state), tooltip: tooltip(state), items: buildItems(state) });
    } catch {
      // A transient failure must never escape the interval.
    }
  };

  void tick();
  setInterval(() => void tick(), POLL_MS);
};
