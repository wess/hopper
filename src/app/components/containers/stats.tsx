// Live resource stats for a running container — CPU%, memory, network, block
// IO. Subscribes to the stats stream on mount and stops it on unmount.

import { useEffect, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { ContainerStats } from "../../../shared/types.ts";
import { bytes, percent } from "../../lib/format.ts";
import { invoke, useEvent } from "../../lib/ipc.ts";

const tone = (p: number): "green" | "amber" | "red" =>
  p >= 85 ? "red" : p >= 60 ? "amber" : "green";

const Meter = ({ pct }: { pct: number }) => (
  <div className="meter">
    <div
      className="meter-fill"
      data-tone={tone(pct)}
      style={{ width: `${Math.min(100, Math.max(0, pct))}%` }}
    />
  </div>
);

export const Stats = ({ id }: { id: string }) => {
  const [s, setS] = useState<ContainerStats | null>(null);

  useEffect(() => {
    setS(null);
    invoke(ch.statsStart, { id }).catch(() => {});
    return () => {
      invoke(ch.statsStop, { id }).catch(() => {});
    };
  }, [id]);

  useEvent<ContainerStats>(ch.statsSample, (sample) => {
    if (sample.id === id) setS(sample);
  });

  if (!s) return <div className="cell-sub">Waiting for samples…</div>;

  return (
    <div className="stat-grid" style={{ padding: 0 }}>
      <div className="stat-card">
        <div className="stat-label">CPU</div>
        <div className="stat-value">{percent(s.cpuPercent)}</div>
        <Meter pct={s.cpuPercent} />
      </div>
      <div className="stat-card">
        <div className="stat-label">Memory</div>
        <div className="stat-value">{percent(s.memPercent)}</div>
        <div className="stat-sub">
          {bytes(s.memUsage)} / {bytes(s.memLimit)}
        </div>
        <Meter pct={s.memPercent} />
      </div>
      <div className="stat-card">
        <div className="stat-label">Network</div>
        <div className="stat-value-sm">
          ↓ {bytes(s.netRx)} · ↑ {bytes(s.netTx)}
        </div>
      </div>
      <div className="stat-card">
        <div className="stat-label">Block IO</div>
        <div className="stat-value-sm">
          R {bytes(s.blockRead)} · W {bytes(s.blockWrite)}
        </div>
        <div className="stat-sub">{s.pids} PIDs</div>
      </div>
    </div>
  );
};
