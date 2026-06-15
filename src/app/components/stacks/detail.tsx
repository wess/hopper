// Stack detail pane. Header with stack-wide lifecycle actions, a services
// table, a compose file editor, and a console that streams action output.

import { toast } from "@basket/ui/toast";
import { Play, RotateCw, Square, Trash2, X } from "lucide-react";
import { useState } from "react";
import type { ComposeAction, ComposeProject } from "../../../shared/types.ts";
import { runComposeAction } from "../../lib/composerun.ts";
import { confirm } from "../../lib/confirm.tsx";
import { formatPorts } from "../../lib/format.ts";
import { Badge, Button, StatusDot } from "../ui.tsx";
import { Console } from "./console.tsx";
import { Editor } from "./editor.tsx";

const STATUS_TONE = { running: "green", partial: "amber", stopped: "neutral" } as const;

type Tab = "services" | "file";

export const Detail = ({
  stack,
  onClose,
  onChanged,
}: {
  stack: ComposeProject;
  onClose: () => void;
  onChanged: () => void;
}) => {
  const [tab, setTab] = useState<Tab>("services");
  const [busy, setBusy] = useState<ComposeAction | null>(null);
  const [log, setLog] = useState<string[]>([]);

  // up needs the compose file(s); the rest operate label-driven by project.
  const target = { files: [...stack.configFiles], project: stack.name };
  const canUp = stack.configFiles.length > 0;

  const act = async (action: ComposeAction) => {
    if (action === "remove" || action === "down") {
      const ok = await confirm({
        title: action === "remove" ? `Remove stack "${stack.name}"?` : `Take "${stack.name}" down?`,
        message:
          action === "remove"
            ? "Stops and removes the stack's containers, networks, and named volumes. This cannot be undone."
            : "Stops and removes the stack's containers and networks. Named volumes are kept.",
        confirmLabel: action === "remove" ? "Remove" : "Down",
        danger: true,
      });
      if (!ok) return;
    }
    setBusy(action);
    setLog([]);
    try {
      const res = await runComposeAction(
        action,
        action === "up" ? target : { project: stack.name },
        undefined,
        (line) => setLog((prev) => [...prev, line]),
      );
      if (res.ok) toast.success(`Stack ${action}`);
      else toast.error(`Compose ${action} failed`, { description: res.error });
      onChanged();
    } finally {
      setBusy(null);
    }
  };

  const running = busy !== null;

  return (
    <div className="detail">
      <div className="detail-head">
        <div className="detail-title">
          <StatusDot
            state={
              stack.status === "running"
                ? "running"
                : stack.status === "partial"
                  ? "restarting"
                  : "exited"
            }
          />
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
            {stack.name}
          </span>
          <Badge tone={STATUS_TONE[stack.status]}>
            {stack.running}/{stack.total} up
          </Badge>
        </div>
        <div className="detail-actions">
          <button
            type="button"
            className="icon-btn"
            title={canUp ? "Up" : "No compose file recorded for this stack"}
            disabled={running || !canUp}
            onClick={() => act("up")}
          >
            <Play size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Start"
            disabled={running}
            onClick={() => act("start")}
          >
            ▶
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Stop"
            disabled={running}
            onClick={() => act("stop")}
          >
            <Square size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Restart"
            disabled={running}
            onClick={() => act("restart")}
          >
            <RotateCw size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            data-danger="true"
            title="Down"
            disabled={running}
            onClick={() => act("down")}
          >
            <Trash2 size={15} />
          </button>
          <Button size="sm" variant="ghost" onClick={onClose} aria-label="Close">
            <X size={15} />
          </Button>
        </div>
      </div>

      <div className="tabs">
        <button
          type="button"
          className="tab"
          data-active={tab === "services"}
          onClick={() => setTab("services")}
        >
          Services
        </button>
        <button
          type="button"
          className="tab"
          data-active={tab === "file"}
          onClick={() => setTab("file")}
        >
          File
        </button>
      </div>

      <div className="tab-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        {tab === "services" ? (
          <div className="table-wrap">
            <table className="table">
              <thead>
                <tr>
                  <th>Service</th>
                  <th>Container</th>
                  <th>Status</th>
                  <th>Image</th>
                  <th>Ports</th>
                </tr>
              </thead>
              <tbody>
                {stack.services.map((s) => (
                  <tr key={s.containerId}>
                    <td>{s.service}</td>
                    <td className="cell-sub">{s.containerName}</td>
                    <td>
                      <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                        <StatusDot state={s.state} />
                        {s.status}
                      </span>
                    </td>
                    <td className="cell-sub">{s.image}</td>
                    <td className="cell-sub">{formatPorts(s.ports)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Editor project={stack.name} files={stack.configFiles} />
        )}

        <Console lines={log} />
      </div>
    </div>
  );
};
