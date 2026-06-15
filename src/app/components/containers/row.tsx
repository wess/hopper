// A single container row in the list table: select checkbox, name + status,
// image, status text, ports, created, and inline lifecycle action buttons.

import { Play, RotateCw, Square, Trash2 } from "lucide-react";
import type { Container } from "../../../shared/types.ts";
import { confirm } from "../../lib/confirm.tsx";
import { ago, formatPorts } from "../../lib/format.ts";
import { StatusDot } from "../ui.tsx";
import * as act from "./actions.ts";

const stop = (e: React.MouseEvent) => e.stopPropagation();

export const Row = ({
  container: c,
  selected,
  active,
  onToggle,
  onOpen,
  onRemoved,
  indent,
}: {
  container: Container;
  selected: boolean;
  active: boolean;
  onToggle: (id: string) => void;
  onOpen: (c: Container) => void;
  onRemoved: () => void;
  indent?: boolean;
}) => {
  const running = c.state === "running";
  const paused = c.state === "paused";

  const remove = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (
      !(await confirm({
        title: `Remove container "${c.name}"?`,
        confirmLabel: "Remove",
        danger: true,
      }))
    )
      return;
    if (await act.remove(c)) onRemoved();
  };

  return (
    <tr
      className="clickable"
      data-selected={selected || active}
      onClick={() => onOpen(c)}
      onKeyDown={(e) => {
        if (e.key === "Enter") onOpen(c);
      }}
    >
      {/* biome-ignore lint/a11y/useKeyWithClickEvents: cell only stops the row click. */}
      <td onClick={stop} style={{ width: 32 }}>
        <input type="checkbox" checked={selected} onChange={() => onToggle(c.id)} />
      </td>
      <td>
        <div
          style={{ display: "flex", alignItems: "center", gap: 9, paddingLeft: indent ? 16 : 0 }}
        >
          <StatusDot state={c.state} />
          <span className="cell-name">{c.name}</span>
        </div>
      </td>
      <td className="cell-mono">{c.image}</td>
      <td className="cell-sub">{c.status}</td>
      <td className="cell-mono">{formatPorts(c.ports) || "—"}</td>
      <td className="cell-sub">{ago(c.created)}</td>
      <td>
        {/* biome-ignore lint/a11y/useKeyWithClickEvents: actions cell only stops row click. */}
        {/* biome-ignore lint/a11y/noStaticElementInteractions: stop-propagation wrapper. */}
        <div className="cell-actions" onClick={stop}>
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
          <button
            type="button"
            className="icon-btn"
            data-danger="true"
            title="Remove"
            onClick={remove}
          >
            <Trash2 size={15} />
          </button>
        </div>
      </td>
    </tr>
  );
};
