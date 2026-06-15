// Stacks view — first-class Docker Compose management. Lists compose projects
// (reconstructed from container labels, so it works against any engine), with a
// search box, quick lifecycle actions, a detail pane (services + file editor),
// and an Up dialog with the full set of compose options.

import { toast } from "@basket/ui/toast";
import { Search, SquareStack } from "lucide-react";
import { useMemo, useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { ComposeAction, ComposeProject } from "../../shared/types.ts";
import { runComposeAction } from "../lib/composerun.ts";
import { confirm } from "../lib/confirm.tsx";
import { useEvent, useLoad } from "../lib/ipc.ts";
import { ComposeDialog } from "./compose/dialog.tsx";
import { Detail } from "./stacks/detail.tsx";
import { Row } from "./stacks/row.tsx";
import { Button, EmptyState, Input, Spinner, ViewHeader } from "./ui.tsx";

export const StacksView = () => {
  const { data, loading, reload } = useLoad(ch.composeList, undefined, []);
  const { data: composeOk } = useLoad(ch.composeAvailable, undefined, []);
  const [query, setQuery] = useState("");
  const [openName, setOpenName] = useState<string | null>(null);
  const [showUp, setShowUp] = useState(false);

  useEvent(ch.resourcesChanged, (e) => {
    if (e.kind === "container") reload();
  });

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data ?? []).filter((s) => !q || s.name.toLowerCase().includes(q));
  }, [data, query]);

  const open = useMemo(
    () => (openName ? ((data ?? []).find((s) => s.name === openName) ?? null) : null),
    [data, openName],
  );

  const quickAction = async (stack: ComposeProject, action: ComposeAction) => {
    if (action === "down") {
      const ok = await confirm({
        title: `Take "${stack.name}" down?`,
        message: "Stops and removes the stack's containers and networks. Named volumes are kept.",
        confirmLabel: "Down",
        danger: true,
      });
      if (!ok) return;
    }
    const res = await runComposeAction(action, { project: stack.name }, undefined, () => {});
    if (res.ok) toast.success(`Stack ${action}`);
    else toast.error(`Compose ${action} failed`, { description: res.error });
    reload();
  };

  const list = (
    <div className="split-list">
      <ViewHeader title="Stacks" count={data?.length}>
        {composeOk ? (
          <Button variant="primary" onClick={() => setShowUp(true)}>
            + Compose up
          </Button>
        ) : null}
      </ViewHeader>

      {composeOk === false ? (
        <div className="toolbar">
          <span className="cell-sub">
            Compose CLI not found — install Docker Compose to bring stacks up; existing stacks are
            still listed and can be stopped.
          </span>
        </div>
      ) : null}

      <div className="toolbar">
        <div className="search-input">
          <Search size={15} />
          <Input
            placeholder="Search stacks…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      </div>

      {loading && !data ? (
        <Spinner label="Loading stacks…" />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<SquareStack size={40} />}
          title={data && data.length > 0 ? "No matching stacks" : "No compose stacks"}
          hint={
            data && data.length > 0
              ? "Try a different search."
              : "Bring a compose file up to create your first stack."
          }
          action={
            composeOk ? (
              <Button variant="primary" onClick={() => setShowUp(true)}>
                + Compose up
              </Button>
            ) : undefined
          }
        />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Stack</th>
                <th>Status</th>
                <th>File</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {filtered.map((s) => (
                <Row
                  key={s.name}
                  stack={s}
                  active={openName === s.name}
                  onOpen={(x) => setOpenName(x.name)}
                  onAction={quickAction}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );

  return (
    <>
      {open ? (
        <div className="split">
          {list}
          <Detail stack={open} onClose={() => setOpenName(null)} onChanged={reload} />
        </div>
      ) : (
        list
      )}
      {showUp ? (
        <ComposeDialog
          onClose={() => setShowUp(false)}
          onDone={reload}
          initialFiles={open?.configFiles}
          initialProject={open?.name}
        />
      ) : null}
    </>
  );
};
