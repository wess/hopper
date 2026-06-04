// Global app state — current view, engine status, and persisted shell prefs
// (theme, sidebar collapse, workspaces). A tiny external store with a React
// hook; the persisted slice is mirrored to localStorage. No deps, no classes.

import { useSyncExternalStore } from "react";
import type { EngineStatus, Workspace } from "../../shared/types.ts";

export type ViewId = "dashboard" | "containers" | "images" | "volumes" | "networks" | "settings";
export type ThemePref = "light" | "dark" | "system";

export type AppState = {
  readonly view: ViewId;
  readonly engine: EngineStatus;
  readonly theme: ThemePref;
  readonly sidebarCollapsed: boolean;
  readonly workspaces: readonly Workspace[];
  readonly activeWorkspace: string; // workspace id; "all" = no filter
};

// The slice we persist across launches.
type Persisted = Pick<AppState, "theme" | "sidebarCollapsed" | "workspaces" | "activeWorkspace">;

const STORAGE_KEY = "hopper:prefs";

const DEFAULTS: Persisted = {
  theme: "system",
  sidebarCollapsed: false,
  workspaces: [],
  activeWorkspace: "all",
};

const loadPersisted = (): Persisted => {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULTS;
    const parsed = JSON.parse(raw) as Partial<Persisted>;
    return {
      theme: parsed.theme ?? DEFAULTS.theme,
      sidebarCollapsed: parsed.sidebarCollapsed ?? DEFAULTS.sidebarCollapsed,
      workspaces: parsed.workspaces ?? DEFAULTS.workspaces,
      activeWorkspace: parsed.activeWorkspace ?? DEFAULTS.activeWorkspace,
    };
  } catch {
    return DEFAULTS;
  }
};

const persisted = loadPersisted();

let state: AppState = {
  view: "containers",
  engine: { connected: false, message: "Connecting…" },
  ...persisted,
};

const listeners = new Set<() => void>();
const emit = () => {
  for (const l of listeners) l();
};

const save = (): void => {
  try {
    const slice: Persisted = {
      theme: state.theme,
      sidebarCollapsed: state.sidebarCollapsed,
      workspaces: state.workspaces,
      activeWorkspace: state.activeWorkspace,
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(slice));
  } catch {
    // storage unavailable — prefs just won't persist this session
  }
};

export const setState = (patch: Partial<AppState>): void => {
  state = { ...state, ...patch };
  save();
  emit();
};

export const setView = (view: ViewId): void => setState({ view });
export const setTheme = (theme: ThemePref): void => setState({ theme });
export const toggleSidebar = (): void => setState({ sidebarCollapsed: !state.sidebarCollapsed });
export const setActiveWorkspace = (id: string): void => setState({ activeWorkspace: id });

export const saveWorkspace = (ws: Workspace): void => {
  const existing = state.workspaces.findIndex((w) => w.id === ws.id);
  const workspaces =
    existing >= 0
      ? state.workspaces.map((w) => (w.id === ws.id ? ws : w))
      : [...state.workspaces, ws];
  setState({ workspaces });
};

export const deleteWorkspace = (id: string): void => {
  setState({
    workspaces: state.workspaces.filter((w) => w.id !== id),
    activeWorkspace: state.activeWorkspace === id ? "all" : state.activeWorkspace,
  });
};

const subscribe = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const useApp = (): AppState => useSyncExternalStore(subscribe, () => state);

// Read the active workspace object, or null for the built-in "all" scope.
export const useActiveWorkspace = (): Workspace | null => {
  const { workspaces, activeWorkspace } = useApp();
  if (activeWorkspace === "all") return null;
  return workspaces.find((w) => w.id === activeWorkspace) ?? null;
};
