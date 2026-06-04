// Generic read-only inspector for the opaque `InspectResult` JSON the daemon
// returns. Renders a collapsible, syntax-tinted tree — used by every detail
// pane's "Inspect" tab.

import { useState } from "react";
import type { InspectResult } from "../../shared/types.ts";

const Value = ({ value }: { value: unknown }) => {
  if (value === null) return <span className="json-null">null</span>;
  if (typeof value === "string") return <span className="json-string">"{value}"</span>;
  if (typeof value === "number") return <span className="json-number">{value}</span>;
  if (typeof value === "boolean") return <span className="json-bool">{String(value)}</span>;
  return null;
};

const Node = ({ name, value, depth }: { name: string; value: unknown; depth: number }) => {
  const isObject = value !== null && typeof value === "object";
  const isArray = Array.isArray(value);
  const [open, setOpen] = useState(depth < 1);

  if (!isObject) {
    return (
      <div className="json-row" style={{ paddingLeft: depth * 14 }}>
        <span className="json-key">{name}</span>
        <span className="json-colon">:</span>
        <Value value={value} />
      </div>
    );
  }

  const entries = isArray
    ? (value as unknown[]).map((v, i) => [String(i), v] as const)
    : Object.entries(value as Record<string, unknown>);
  const count = entries.length;

  return (
    <div>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: collapsible JSON node, click toggles */}
      {/* biome-ignore lint/a11y/useKeyWithClickEvents: read-only inspector, pointer-driven */}
      <div
        className="json-row json-branch"
        style={{ paddingLeft: depth * 14 }}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="json-caret" data-open={open}>
          ▸
        </span>
        <span className="json-key">{name}</span>
        <span className="json-colon">:</span>
        <span className="json-meta">{isArray ? `[${count}]` : `{${count}}`}</span>
      </div>
      {open ? entries.map(([k, v]) => <Node key={k} name={k} value={v} depth={depth + 1} />) : null}
    </div>
  );
};

export const Inspect = ({ data }: { data: InspectResult }) => (
  <div className="json-tree">
    {Object.entries(data).map(([k, v]) => (
      <Node key={k} name={k} value={v} depth={0} />
    ))}
  </div>
);
