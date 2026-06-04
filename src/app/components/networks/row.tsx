// A single network row in the Networks table.

import { Trash2 } from "lucide-react";
import type { MouseEvent } from "react";
import type { Network } from "../../../shared/types.ts";
import { Badge } from "../ui.tsx";
import { isBuiltin } from "./builtin.ts";

type Props = {
  readonly network: Network;
  readonly onOpen: () => void;
  readonly onDelete: () => void;
};

export const NetworkRow = ({ network, onOpen, onDelete }: Props) => {
  const builtin = isBuiltin(network.name);
  const subnet = network.ipam[0]?.subnet ?? "—";
  const del = (e: MouseEvent) => {
    e.stopPropagation();
    onDelete();
  };
  return (
    <tr className="clickable" onClick={onOpen}>
      <td>
        <div className="cell-name">{network.name}</div>
      </td>
      <td className="cell-sub">{network.driver}</td>
      <td className="cell-sub">{network.scope}</td>
      <td>
        <div className="chips">
          {network.internal ? <Badge tone="amber">Internal</Badge> : null}
          {network.attachable ? <Badge tone="blue">Attachable</Badge> : null}
        </div>
      </td>
      <td className="cell-mono">{subnet}</td>
      <td className="right cell-sub">{network.containers}</td>
      <td className="right">
        <div className="cell-actions">
          <button
            type="button"
            className="icon-btn"
            data-danger="true"
            title={builtin ? "Built-in network" : "Delete"}
            onClick={del}
            disabled={builtin}
          >
            <Trash2 size={15} />
          </button>
        </div>
      </td>
    </tr>
  );
};
