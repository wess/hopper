import { Palette, type PaletteCommand, usePaletteShortcut } from "@basket/ui/palette";
import { Toaster } from "@basket/ui/toast";
import type { ReactElement } from "react";
import { useCallback, useEffect, useState } from "react";
import * as ch from "../shared/channels.ts";
import { ContainersView } from "./components/containers.tsx";
import { DashboardView } from "./components/dashboard.tsx";
import { ImagesView } from "./components/images.tsx";
import { NetworksView } from "./components/networks.tsx";
import { SettingsView } from "./components/settings.tsx";
import { Sidebar } from "./components/sidebar.tsx";
import { VolumesView } from "./components/volumes.tsx";
import { subscribe } from "./lib/ipc.ts";
import { setState, setTheme, setView, type ThemePref, useApp, type ViewId } from "./state/store.ts";

// Resolve a theme preference to a concrete light/dark value, following the OS
// when set to "system".
const resolveTheme = (pref: ThemePref): "light" | "dark" => {
  if (pref === "system") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return pref;
};

const applyTheme = (resolved: "light" | "dark"): void => {
  document.documentElement.setAttribute("data-theme", resolved);
  document.documentElement.setAttribute("data-basket-theme", resolved);
};

// ⌘⇧L cycles through the three preferences.
const nextTheme = (pref: ThemePref): ThemePref =>
  pref === "light" ? "dark" : pref === "dark" ? "system" : "light";

const VIEWS: Record<ViewId, () => ReactElement> = {
  dashboard: DashboardView,
  containers: ContainersView,
  images: ImagesView,
  volumes: VolumesView,
  networks: NetworksView,
  settings: SettingsView,
};

export const App = () => {
  const { view, engine, theme, sidebarCollapsed } = useApp();
  const [paletteOpen, setPaletteOpen] = useState(false);

  // Apply the resolved theme, and while on "system" track OS changes live.
  useEffect(() => {
    applyTheme(resolveTheme(theme));
    if (theme !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme(resolveTheme("system"));
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);

  const cycleTheme = useCallback(() => setTheme(nextTheme(theme)), [theme]);

  usePaletteShortcut(() => setPaletteOpen((v) => !v));

  // Engine status: prime once, then follow host broadcasts.
  useEffect(() => {
    void import("./lib/ipc.ts").then(({ invoke }) =>
      invoke(ch.enginePing, undefined)
        .then((s) => setState({ engine: s }))
        .catch(() => {}),
    );
    return subscribe(ch.engineStatusChanged, (s) => setState({ engine: s }));
  }, []);

  // Tray (status-bar app) navigation requests.
  useEffect(
    () =>
      subscribe(ch.trayNavigate, (v) => {
        if (
          v === "dashboard" ||
          v === "containers" ||
          v === "images" ||
          v === "volumes" ||
          v === "networks" ||
          v === "settings"
        ) {
          setView(v);
        }
      }),
    [],
  );

  // Native menu commands.
  useEffect(
    () =>
      subscribe(ch.runCommand, (cmd) => {
        if (cmd === "theme") cycleTheme();
        else if (cmd === "palette") setPaletteOpen(true);
        else if (
          cmd === "dashboard" ||
          cmd === "containers" ||
          cmd === "images" ||
          cmd === "volumes" ||
          cmd === "networks" ||
          cmd === "settings"
        ) {
          setView(cmd);
        } else if (cmd === "run" || cmd === "pull") {
          setView("images");
          window.dispatchEvent(new CustomEvent(`hopper:${cmd}`));
        } else if (cmd === "prune") {
          setView("dashboard");
          window.dispatchEvent(new CustomEvent("hopper:prune"));
        }
      }),
    [cycleTheme],
  );

  const commands: PaletteCommand[] = [
    { id: "dashboard", label: "Go to Dashboard", action: () => setView("dashboard") },
    { id: "containers", label: "Go to Containers", action: () => setView("containers") },
    { id: "images", label: "Go to Images", action: () => setView("images") },
    { id: "volumes", label: "Go to Volumes", action: () => setView("volumes") },
    { id: "networks", label: "Go to Networks", action: () => setView("networks") },
    { id: "settings", label: "Go to Settings", action: () => setView("settings") },
    {
      id: "pull",
      label: "Pull Image…",
      action: () => {
        setView("images");
        window.dispatchEvent(new CustomEvent("hopper:pull"));
      },
    },
    {
      id: "run",
      label: "Run Container…",
      action: () => {
        setView("images");
        window.dispatchEvent(new CustomEvent("hopper:run"));
      },
    },
    { id: "theme", label: "Cycle Theme (light / dark / system)", hint: "⌘⇧L", action: cycleTheme },
  ];

  const Current = VIEWS[view];

  return (
    <div className="app-shell" data-collapsed={sidebarCollapsed}>
      <Sidebar />
      <main className="app-main">
        {!engine.connected ? (
          <div className="engine-banner">
            <span className="engine-banner-dot" />
            {engine.message} — start Docker Desktop to continue.
          </div>
        ) : null}
        <div className="app-content">
          <Current />
        </div>
      </main>
      <Palette open={paletteOpen} onClose={() => setPaletteOpen(false)} commands={commands} />
      <Toaster />
    </div>
  );
};
