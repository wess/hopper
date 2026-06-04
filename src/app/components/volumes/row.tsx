// A single volume row in the Volumes table.

import { Trash2 } from "lucide-react";
import type { MouseEvent } from "react";
import type { Volume } from "../../../shared/types.ts";
import { agoIso, bytes } from "../../lib/format.ts";
import { Badge } from "../ui.tsx";

type Props = {
  readonly volume: Volume;
  readonly onOpen: () => void;
  readonly onDelete: () => void;
};

export const VolumeRow = ({ volume, onOpen, onDelete }: Props) => {
  const del = (e: MouseEvent) => {
    e.stopPropagation();
    onDelete();
  };
  return (
    <tr className="clickable" onClick={onOpen}>
      <td>
        <div className="cell-name">{volume.name}</div>
      </td>
      <td className="cell-sub">{volume.driver}</td>
      <td>
        <Badge tone={volume.inUse ? "green" : "neutral"}>
          {volume.inUse ? "In use" : "Unused"}
        </Badge>
      </td>
      <td className="right cell-sub">{bytes(volume.size)}</td>
      <td className="right cell-sub" style={{ whiteSpace: "nowrap" }}>
        {agoIso(volume.created)}
      </td>
      <td className="right">
        <div className="cell-actions">
          <button
            type="button"
            className="icon-btn"
            data-danger="true"
            title="Delete"
            onClick={del}
          >
            <Trash2 size={15} />
          </button>
        </div>
      </td>
    </tr>
  );
};
