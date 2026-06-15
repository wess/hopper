// Containers view — Docker Desktop's Containers tab. Lists all containers
// (with compose-stack grouping), a search/filter toolbar, multi-select bulk
// actions, a Run dialog, prune, and a tabbed detail pane.

import { toast } from "@basket/ui/toast";
import { Boxes, ChevronDown, ChevronRight, Layers, Search } from "lucide-react";
import { Fragment, useMemo, useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { Container } from "../../shared/types.ts";
import { stackState } from "../lib/compose.ts";
import { confirm } from "../lib/confirm.tsx";
import { bytes } from "../lib/format.ts";
import { errorMessage, invoke, useEvent, useLoad } from "../lib/ipc.ts";
import { matchesWorkspace } from "../lib/workspaces.ts";
import { useActiveWorkspace } from "../state/store.ts";
import { ComposeDialog } from "./compose/dialog.tsx";
import * as act from "./containers/actions.ts";
import { Detail } from "./containers/detail.tsx";
import { Row } from "./containers/row.tsx";
import { RunDialog } from "./rundialog.tsx";
import { Badge, Button, EmptyState, Input, Spinner, Toolbar, ViewHeader } from "./ui.tsx";

type Group = { readonly key: string; readonly project: string | null; readonly items: Container[] };

const STACK_TONE = { running: "green", partial: "amber", stopped: "neutral" } as const;

const group = (list: readonly Container[]): Group[] => {
  const standalone: Container[] = [];
  const byProject = new Map<string, Container[]>();
  for (const c of list) {
    if (c.composeProject) {
      const arr = byProject.get(c.composeProject) ?? [];
      arr.push(c);
      byProject.set(c.composeProject, arr);
    } else {
      standalone.push(c);
    }
  }
  const groups: Group[] = [];
  for (const [project, items] of byProject) groups.push({ key: `proj:${project}`, project, items });
  groups.sort((a, b) => (a.project ?? "").localeCompare(b.project ?? ""));
  if (standalone.length) groups.push({ key: "standalone", project: null, items: standalone });
  return groups;
};

export const ContainersView = () => {
  const { data, loading, reload } = useLoad(ch.containerList, { all: true }, []);
  const [query, setQuery] = useState("");
  const [onlyRunning, setOnlyRunning] = useState(false);
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [openId, setOpenId] = useState<string | null>(null);
  const [showRun, setShowRun] = useState(false);
  const [showCompose, setShowCompose] = useState(false);
  const [pruning, setPruning] = useState(false);

  const workspace = useActiveWorkspace();
  const { data: composeOk } = useLoad(ch.composeAvailable, undefined, []);

  useEvent(ch.resourcesChanged, (e) => {
    if (e.kind === "container") reload();
  });

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (data ?? []).filter((c) => {
      if (!matchesWorkspace(c, workspace)) return false;
      if (onlyRunning && c.state !== "running") return false;
      if (!q) return true;
      return c.name.toLowerCase().includes(q) || c.image.toLowerCase().includes(q);
    });
  }, [data, query, onlyRunning, workspace]);

  const groups = useMemo(() => group(filtered), [filtered]);
  const open = useMemo(
    () => (openId ? ((data ?? []).find((c) => c.id === openId) ?? null) : null),
    [data, openId],
  );

  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleGroup = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const bulk = async (op: "start" | "stop" | "restart" | "remove") => {
    const ids = [...selected];
    if (
      op === "remove" &&
      !(await confirm({
        title: `Remove ${ids.length} container(s)?`,
        confirmLabel: "Remove",
        danger: true,
      }))
    )
      return;
    await act.batch(ids, op);
    setSelected(new Set());
  };

  // Apply a lifecycle op to every container in a compose stack.
  const stackAction = async (g: Group, op: "start" | "stop" | "restart" | "remove") => {
    if (
      op === "remove" &&
      !(await confirm({
        title: `Remove stack "${g.project}" (${g.items.length})?`,
        confirmLabel: "Remove",
        danger: true,
      }))
    ) {
      return;
    }
    await act.batch(
      g.items.map((c) => c.id),
      op,
    );
  };

  const prune = async () => {
    if (
      !(await confirm({
        title: "Remove all stopped containers?",
        confirmLabel: "Prune",
        danger: true,
      }))
    )
      return;
    setPruning(true);
    try {
      const r = await invoke(ch.containerPrune, undefined);
      toast.success(`Pruned ${r.removed} container(s)`, {
        description: `Reclaimed ${bytes(r.reclaimed)}`,
      });
    } catch (e) {
      toast.error("Prune failed", { description: errorMessage(e) });
    } finally {
      setPruning(false);
    }
  };

  const list = (
    <div className="split-list">
      <ViewHeader title="Containers" count={data?.length}>
        {composeOk ? (
          <Button onClick={() => setShowCompose(true)}>
            <Layers size={15} /> Compose
          </Button>
        ) : null}
        <Button variant="primary" onClick={() => setShowRun(true)}>
          + Run
        </Button>
        <Button onClick={prune} disabled={pruning}>
          {pruning ? "Pruning…" : "Prune"}
        </Button>
      </ViewHeader>

      <div className="toolbar">
        <div className="search-input">
          <Search size={15} />
          <Input
            placeholder="Search name or image…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <Button
          size="sm"
          variant={onlyRunning ? "primary" : "default"}
          onClick={() => setOnlyRunning((v) => !v)}
        >
          Only running
        </Button>
        <Toolbar.Spacer />
      </div>

      {selected.size > 0 ? (
        <div className="toolbar" style={{ paddingTop: 0 }}>
          <span className="cell-sub">{selected.size} selected</span>
          <Toolbar.Spacer />
          <Button size="sm" onClick={() => bulk("start")}>
            Start
          </Button>
          <Button size="sm" onClick={() => bulk("stop")}>
            Stop
          </Button>
          <Button size="sm" onClick={() => bulk("restart")}>
            Restart
          </Button>
          <Button size="sm" variant="danger" onClick={() => bulk("remove")}>
            Remove
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
        </div>
      ) : null}

      {loading && !data ? (
        <Spinner label="Loading containers…" />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<Boxes size={40} />}
          title={data && data.length > 0 ? "No matching containers" : "No containers yet"}
          hint={
            data && data.length > 0
              ? "Try adjusting your search or filters."
              : "Run an image to create your first container."
          }
          action={
            <Button variant="primary" onClick={() => setShowRun(true)}>
              + Run a container
            </Button>
          }
        />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th />
                <th>Name</th>
                <th>Image</th>
                <th>Status</th>
                <th>Ports</th>
                <th>Created</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {groups.map((g) => {
                const isCollapsed = collapsed.has(g.key);
                const ss = g.project ? stackState(g.items) : null;
                const stackBtn =
                  (op: "start" | "stop" | "restart" | "remove") => (e: React.MouseEvent) => {
                    e.stopPropagation();
                    void stackAction(g, op);
                  };
                return (
                  <Fragment key={g.key}>
                    {g.project && ss ? (
                      <tr
                        className="group-row"
                        onClick={() => toggleGroup(g.key)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") toggleGroup(g.key);
                        }}
                      >
                        <td colSpan={7}>
                          <div className="group-head">
                            <span className="chev">
                              {isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
                            </span>
                            <span className="group-name">{g.project}</span>
                            <Badge tone={STACK_TONE[ss.state]}>
                              {ss.running}/{ss.total} up
                            </Badge>
                            <Toolbar.Spacer />
                            <Button size="sm" onClick={stackBtn("start")}>
                              Start
                            </Button>
                            <Button size="sm" onClick={stackBtn("stop")}>
                              Stop
                            </Button>
                            <Button size="sm" onClick={stackBtn("restart")}>
                              Restart
                            </Button>
                            <Button size="sm" variant="danger" onClick={stackBtn("remove")}>
                              Remove
                            </Button>
                          </div>
                        </td>
                      </tr>
                    ) : null}
                    {isCollapsed
                      ? null
                      : g.items.map((c) => (
                          <Row
                            key={c.id}
                            container={c}
                            indent={!!g.project}
                            selected={selected.has(c.id)}
                            active={openId === c.id}
                            onToggle={toggle}
                            onOpen={(x) => setOpenId(x.id)}
                            onRemoved={reload}
                          />
                        ))}
                  </Fragment>
                );
              })}
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
          <Detail
            container={open}
            onClose={() => setOpenId(null)}
            onRemoved={() => {
              setOpenId(null);
              reload();
            }}
          />
        </div>
      ) : (
        list
      )}
      {showRun ? (
        <RunDialog
          onClose={() => setShowRun(false)}
          onLaunched={() => {
            toast.success("Container started");
            reload();
          }}
        />
      ) : null}
      {showCompose ? <ComposeDialog onClose={() => setShowCompose(false)} onDone={reload} /> : null}
    </>
  );
};
