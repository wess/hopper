// Left navigation rail. A drag region (leaving room for the macOS traffic
// lights), a workspace switcher, the view nav, and the engine-status footer.
// Collapses to an icon-only rail.

import {
  Boxes,
  Check,
  ChevronsUpDown,
  HardDrive,
  Layers,
  LayoutDashboard,
  Network,
  Package,
  PanelLeft,
  PanelLeftClose,
  Settings,
} from "lucide-react";
import { type ComponentType, useEffect, useRef, useState } from "react";
import { setActiveWorkspace, setView, toggleSidebar, useApp, type ViewId } from "../state/store.ts";

type NavItem = {
  readonly id: ViewId;
  readonly label: string;
  readonly icon: ComponentType<{ size?: number }>;
};

const NAV: readonly NavItem[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "containers", label: "Containers", icon: Boxes },
  { id: "images", label: "Images", icon: Package },
  { id: "volumes", label: "Volumes", icon: HardDrive },
  { id: "networks", label: "Networks", icon: Network },
];

// Workspace switcher popover — pick the active scope or jump to Settings to
// manage them.
const WorkspaceSwitcher = ({ collapsed }: { collapsed: boolean }) => {
  const { workspaces, activeWorkspace } = useApp();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const active = workspaces.find((w) => w.id === activeWorkspace);
  const label = active ? active.name : "All resources";

  if (collapsed) {
    return (
      <button
        type="button"
        className="nav-ws-collapsed"
        title={`Workspace: ${label}`}
        onClick={toggleSidebar}
      >
        <Layers size={16} />
        {active ? <span className="nav-ws-dot" /> : null}
      </button>
    );
  }

  return (
    <div className="nav-ws" ref={ref}>
      <button type="button" className="nav-ws-btn" onClick={() => setOpen((v) => !v)}>
        <Layers size={14} />
        <span className="nav-ws-name">{label}</span>
        <ChevronsUpDown size={13} className="nav-ws-chev" />
      </button>
      {open ? (
        <div className="nav-ws-menu">
          <button
            type="button"
            className="nav-ws-opt"
            onClick={() => {
              setActiveWorkspace("all");
              setOpen(false);
            }}
          >
            <span>All resources</span>
            {activeWorkspace === "all" ? <Check size={14} /> : null}
          </button>
          {workspaces.map((w) => (
            <button
              type="button"
              key={w.id}
              className="nav-ws-opt"
              onClick={() => {
                setActiveWorkspace(w.id);
                setOpen(false);
              }}
            >
              <span>{w.name}</span>
              {activeWorkspace === w.id ? <Check size={14} /> : null}
            </button>
          ))}
          <div className="nav-ws-sep" />
          <button
            type="button"
            className="nav-ws-opt nav-ws-manage"
            onClick={() => {
              setView("settings");
              setOpen(false);
            }}
          >
            Manage workspaces…
          </button>
        </div>
      ) : null}
    </div>
  );
};

export const Sidebar = () => {
  const { view, engine, sidebarCollapsed } = useApp();
  return (
    <nav className="nav" data-collapsed={sidebarCollapsed}>
      <div className="nav-top">
        <button
          type="button"
          className="nav-collapse"
          title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
          onClick={toggleSidebar}
        >
          {sidebarCollapsed ? <PanelLeft size={16} /> : <PanelLeftClose size={16} />}
        </button>
      </div>

      <WorkspaceSwitcher collapsed={sidebarCollapsed} />

      <div className="nav-items">
        {NAV.map(({ id, label, icon: Icon }) => (
          <button
            type="button"
            key={id}
            className="nav-item"
            data-active={view === id}
            title={sidebarCollapsed ? label : undefined}
            onClick={() => setView(id)}
          >
            <Icon size={17} />
            <span className="nav-label">{label}</span>
          </button>
        ))}
      </div>

      <div className="nav-footer">
        <button
          type="button"
          className="nav-item"
          data-active={view === "settings"}
          title={sidebarCollapsed ? "Settings" : undefined}
          onClick={() => setView("settings")}
        >
          <Settings size={17} />
          <span className="nav-label">Settings</span>
        </button>
        <div className="engine-status" data-connected={engine.connected} title={engine.message}>
          <span className="engine-dot" />
          <span className="engine-text nav-label">
            {engine.connected ? "Engine running" : "Engine offline"}
          </span>
        </div>
      </div>
    </nav>
  );
};
