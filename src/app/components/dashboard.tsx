// Dashboard — Docker Desktop's home/overview. Headline counts, disk usage with
// a clean-up flow, system info, and a live activity feed. When the engine is
// unreachable we show a friendly banner instead of erroring.

import { PlugZap } from "lucide-react";
import { useApp } from "../state/store.ts";
import { Disk } from "./dashboard/disk.tsx";
import { Feed } from "./dashboard/feed.tsx";
import { Stats } from "./dashboard/stats.tsx";
import { SysInfo } from "./dashboard/sysinfo.tsx";
import { EmptyState, ViewHeader } from "./ui.tsx";

export const DashboardView = () => {
  const { engine } = useApp();

  if (!engine.connected) {
    return (
      <div className="scroll-body">
        <ViewHeader title="Dashboard" />
        <EmptyState
          icon={<PlugZap size={30} />}
          title="Docker engine not connected"
          hint={engine.message || "Start Docker, then this view will populate automatically."}
        />
      </div>
    );
  }

  return (
    <div className="scroll-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <ViewHeader title="Dashboard" />
      <Stats />
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(0, 1.4fr) minmax(0, 1fr)",
          gap: 14,
          padding: "0 22px",
        }}
      >
        <Disk />
        <SysInfo />
      </div>
      <div style={{ padding: "0 22px", display: "flex", minHeight: 0 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <Feed />
        </div>
      </div>
    </div>
  );
};
