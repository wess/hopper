// Container detail pane. Header with lifecycle actions plus tabbed views:
// Logs, Inspect, Stats, Terminal, Processes.

import { Pause, Play, RotateCw, Square, Trash2, X } from "lucide-react";
import { useState } from "react";
import type { Container } from "../../../shared/types.ts";
import { ago, formatPorts, shortId } from "../../lib/format.ts";
import { Badge, Button, StatusDot } from "../ui.tsx";
import * as act from "./actions.ts";
import { InspectTab } from "./inspecttab.tsx";
import { Logs } from "./logs.tsx";
import { Processes } from "./processes.tsx";
import { Stats } from "./stats.tsx";
import { Terminal } from "./terminal.tsx";

type Tab = "logs" | "inspect" | "stats" | "terminal" | "processes";
const TABS: readonly { readonly id: Tab; readonly label: string }[] = [
  { id: "logs", label: "Logs" },
  { id: "inspect", label: "Inspect" },
  { id: "stats", label: "Stats" },
  { id: "terminal", label: "Terminal" },
  { id: "processes", label: "Processes" },
];

const stateTone = (state: string): "green" | "amber" | "red" | "neutral" =>
  state === "running"
    ? "green"
    : state === "paused"
      ? "amber"
      : state === "dead"
        ? "red"
        : "neutral";

export const Detail = ({
  container,
  onClose,
  onRemoved,
}: {
  container: Container;
  onClose: () => void;
  onRemoved: () => void;
}) => {
  const [tab, setTab] = useState<Tab>("logs");
  const c = container;
  const running = c.state === "running";
  const paused = c.state === "paused";

  const doRemove = async () => {
    if (!window.confirm(`Remove container "${c.name}"? This cannot be undone.`)) return;
    if (await act.remove(c)) onRemoved();
  };

  return (
    <div className="detail">
      <div className="detail-head">
        <div className="detail-title">
          <StatusDot state={c.state} />
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {c.name}
          </span>
          <span className="detail-title-sub">{shortId(c.id)}</span>
        </div>
        <div className="detail-actions">
          {running ? (
            <button type="button" className="icon-btn" title="Stop" onClick={() => act.stop(c.id)}>
              <Square size={15} />
            </button>
          ) : (
            <button
              type="button"
              className="icon-btn"
              title="Start"
              onClick={() => act.start(c.id)}
            >
              <Play size={15} />
            </button>
          )}
          <button
            type="button"
            className="icon-btn"
            title="Restart"
            disabled={!running && !paused}
            onClick={() => act.restart(c.id)}
          >
            <RotateCw size={15} />
          </button>
          {paused ? (
            <button
              type="button"
              className="icon-btn"
              title="Resume"
              onClick={() => act.unpause(c.id)}
            >
              <Play size={15} />
            </button>
          ) : running ? (
            <button
              type="button"
              className="icon-btn"
              title="Pause"
              onClick={() => act.pause(c.id)}
            >
              <Pause size={15} />
            </button>
          ) : null}
          <button
            type="button"
            className="icon-btn"
            data-danger="true"
            title="Remove"
            onClick={doRemove}
          >
            <Trash2 size={15} />
          </button>
          <Button size="sm" variant="ghost" onClick={onClose} aria-label="Close">
            <X size={15} />
          </Button>
        </div>
      </div>

      <div style={{ padding: "10px 20px", display: "flex", gap: 16, flexWrap: "wrap" }}>
        <Badge tone={stateTone(c.state)}>{c.status}</Badge>
        <span className="cell-sub">{c.image}</span>
        {formatPorts(c.ports) ? <span className="cell-sub">{formatPorts(c.ports)}</span> : null}
        <span className="cell-sub">created {ago(c.created)}</span>
      </div>

      <div className="tabs">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            className="tab"
            data-active={tab === t.id}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "logs" ? (
        <Logs id={c.id} />
      ) : tab === "terminal" ? (
        <Terminal id={c.id} />
      ) : (
        <div className="tab-body">
          {tab === "inspect" ? <InspectTab id={c.id} /> : null}
          {tab === "stats" ? <Stats id={c.id} /> : null}
          {tab === "processes" ? <Processes id={c.id} /> : null}
        </div>
      )}
    </div>
  );
};
