// One stack row in the Stacks list. Aggregate status, service count, the source
// compose file, and quick lifecycle actions.

import type { ComposeAction, ComposeProject } from "../../../shared/types.ts";
import { Badge, Button, StatusDot } from "../ui.tsx";

const STATUS_TONE = { running: "green", partial: "amber", stopped: "neutral" } as const;
const DOT = { running: "running", partial: "restarting", stopped: "exited" } as const;

const basename = (p: string): string => p.split("/").pop() ?? p;

export const Row = ({
  stack,
  active,
  onOpen,
  onAction,
}: {
  stack: ComposeProject;
  active: boolean;
  onOpen: (s: ComposeProject) => void;
  onAction: (s: ComposeProject, action: ComposeAction) => void;
}) => {
  const file = stack.configFiles[0];
  const btn = (action: ComposeAction) => (e: React.MouseEvent) => {
    e.stopPropagation();
    onAction(stack, action);
  };
  return (
    <tr className="row" data-active={active} onClick={() => onOpen(stack)}>
      <td>
        <span style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
          <StatusDot state={DOT[stack.status]} />
          <span className="cell-strong">{stack.name}</span>
        </span>
      </td>
      <td>
        <Badge tone={STATUS_TONE[stack.status]}>
          {stack.running}/{stack.total} up
        </Badge>
      </td>
      <td className="cell-sub" title={file}>
        {file ? basename(file) : "—"}
      </td>
      <td>
        <div className="row-actions">
          <Button size="sm" onClick={btn("start")}>
            Start
          </Button>
          <Button size="sm" onClick={btn("stop")}>
            Stop
          </Button>
          <Button size="sm" variant="danger" onClick={btn("down")}>
            Down
          </Button>
        </div>
      </td>
    </tr>
  );
};
