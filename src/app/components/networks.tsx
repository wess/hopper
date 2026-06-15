// Networks view — list networks with search, create, prune, per-row delete
// (disabled for built-ins), and a detail modal (summary + Inspect + connect).

import { toast } from "@basket/ui/toast";
import { Network as NetworkIcon, Plus, Search, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { Network } from "../../shared/types.ts";
import { confirm } from "../lib/confirm.tsx";
import { bytes } from "../lib/format.ts";
import { errorMessage, invoke, useEvent, useLoad } from "../lib/ipc.ts";
import { isBuiltin } from "./networks/builtin.ts";
import { CreateNetwork } from "./networks/create.tsx";
import { NetworkDetail } from "./networks/detail.tsx";
import { NetworkRow } from "./networks/row.tsx";
import { Button, EmptyState, Spinner, Toolbar, ViewHeader } from "./ui.tsx";

type Dialog = { readonly kind: "create" } | { readonly kind: "detail"; readonly network: Network };

export const NetworksView = () => {
  const { data, error, loading, reload } = useLoad(ch.networkList, undefined);
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<Dialog | null>(null);

  useEvent(ch.resourcesChanged, (e) => {
    if (e.kind === "network") reload();
  });

  const networks = data ?? [];
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return networks;
    return networks.filter((n) => n.name.toLowerCase().includes(q));
  }, [networks, query]);

  const remove = async (network: Network) => {
    if (isBuiltin(network.name)) return;
    if (
      !(await confirm({
        title: `Delete network ${network.name}?`,
        confirmLabel: "Delete",
        danger: true,
      }))
    )
      return;
    try {
      await invoke(ch.networkRemove, { id: network.id });
      toast.success(`Removed ${network.name}`);
      reload();
    } catch (e) {
      toast.error("Remove failed", { description: errorMessage(e) });
    }
  };

  const prune = async () => {
    if (
      !(await confirm({
        title: "Remove all unused networks?",
        message: "This cannot be undone.",
        confirmLabel: "Prune",
        danger: true,
      }))
    )
      return;
    try {
      const report = await invoke(ch.networkPrune, undefined);
      toast.success(`Removed ${report.removed} network(s)`, {
        description: report.reclaimed > 0 ? `Reclaimed ${bytes(report.reclaimed)}` : undefined,
      });
      reload();
    } catch (e) {
      toast.error("Prune failed", { description: errorMessage(e) });
    }
  };

  return (
    <>
      <ViewHeader title="Networks" count={networks.length}>
        <Button onClick={prune}>
          <Trash2 size={15} /> Prune
        </Button>
        <Button variant="primary" onClick={() => setDialog({ kind: "create" })}>
          <Plus size={15} /> Create
        </Button>
      </ViewHeader>

      <Toolbar>
        <div className="search-input">
          <Search size={15} />
          <input
            className="input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search networks…"
          />
        </div>
      </Toolbar>

      {loading ? (
        <Spinner label="Loading networks…" />
      ) : error ? (
        <EmptyState
          title="Couldn’t load networks"
          hint={error}
          action={<Button onClick={reload}>Retry</Button>}
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<NetworkIcon size={32} />}
          title={query ? "No matching networks" : "No networks yet"}
          hint={query ? "Try a different search." : "Create a network to get started."}
          action={
            query ? undefined : (
              <Button variant="primary" onClick={() => setDialog({ kind: "create" })}>
                <Plus size={15} /> Create a network
              </Button>
            )
          }
        />
      ) : (
        <div className="table-wrap">
          <table className="table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Driver</th>
                <th>Scope</th>
                <th>Flags</th>
                <th>Subnet</th>
                <th className="right">Containers</th>
                <th className="right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((network) => (
                <NetworkRow
                  key={network.id}
                  network={network}
                  onOpen={() => setDialog({ kind: "detail", network })}
                  onDelete={() => remove(network)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {dialog?.kind === "create" ? (
        <CreateNetwork onClose={() => setDialog(null)} onCreated={reload} />
      ) : null}
      {dialog?.kind === "detail" ? (
        <NetworkDetail
          network={dialog.network}
          onClose={() => setDialog(null)}
          onChanged={reload}
        />
      ) : null}
    </>
  );
};
