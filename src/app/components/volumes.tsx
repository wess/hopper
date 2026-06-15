// Volumes view — list volumes with search, create, prune, per-row delete, and a
// detail modal (summary + Inspect).

import { toast } from "@basket/ui/toast";
import { HardDrive, Plus, Search, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { Volume } from "../../shared/types.ts";
import { confirm } from "../lib/confirm.tsx";
import { bytes } from "../lib/format.ts";
import { errorMessage, invoke, useEvent, useLoad } from "../lib/ipc.ts";
import { Button, EmptyState, Spinner, Toolbar, ViewHeader } from "./ui.tsx";
import { CreateVolume } from "./volumes/create.tsx";
import { VolumeDetail } from "./volumes/detail.tsx";
import { VolumeRow } from "./volumes/row.tsx";

type Dialog = { readonly kind: "create" } | { readonly kind: "detail"; readonly volume: Volume };

export const VolumesView = () => {
  const { data, error, loading, reload } = useLoad(ch.volumeList, undefined);
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<Dialog | null>(null);

  useEvent(ch.resourcesChanged, (e) => {
    if (e.kind === "volume") reload();
  });

  const volumes = data ?? [];
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return volumes;
    return volumes.filter((v) => v.name.toLowerCase().includes(q));
  }, [volumes, query]);

  const remove = async (volume: Volume) => {
    if (
      !(await confirm({
        title: `Delete volume ${volume.name}?`,
        confirmLabel: "Delete",
        danger: true,
      }))
    )
      return;
    try {
      await invoke(ch.volumeRemove, { name: volume.name });
      toast.success(`Removed ${volume.name}`);
      reload();
    } catch (e) {
      const msg = errorMessage(e);
      if (/in use|conflict|being used/i.test(msg)) {
        if (
          await confirm({
            title: `${volume.name} is in use.`,
            message: "Force remove?",
            confirmLabel: "Force remove",
            danger: true,
          })
        ) {
          try {
            await invoke(ch.volumeRemove, { name: volume.name, force: true });
            toast.success(`Force-removed ${volume.name}`);
            reload();
          } catch (e2) {
            toast.error("Remove failed", { description: errorMessage(e2) });
          }
        }
      } else {
        toast.error("Remove failed", { description: msg });
      }
    }
  };

  const prune = async () => {
    if (
      !(await confirm({
        title: "Remove all unused volumes?",
        message: "This cannot be undone.",
        confirmLabel: "Prune",
        danger: true,
      }))
    )
      return;
    try {
      const report = await invoke(ch.volumePrune, undefined);
      toast.success(`Reclaimed ${bytes(report.reclaimed)}`, {
        description: `${report.removed} volume(s) removed`,
      });
      reload();
    } catch (e) {
      toast.error("Prune failed", { description: errorMessage(e) });
    }
  };

  return (
    <>
      <ViewHeader title="Volumes" count={volumes.length}>
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
            placeholder="Search volumes…"
          />
        </div>
      </Toolbar>

      {loading ? (
        <Spinner label="Loading volumes…" />
      ) : error ? (
        <EmptyState
          title="Couldn’t load volumes"
          hint={error}
          action={<Button onClick={reload}>Retry</Button>}
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<HardDrive size={32} />}
          title={query ? "No matching volumes" : "No volumes yet"}
          hint={query ? "Try a different search." : "Create a volume to get started."}
          action={
            query ? undefined : (
              <Button variant="primary" onClick={() => setDialog({ kind: "create" })}>
                <Plus size={15} /> Create a volume
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
                <th>Status</th>
                <th className="right">Size</th>
                <th className="right">Created</th>
                <th className="right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((volume) => (
                <VolumeRow
                  key={volume.name}
                  volume={volume}
                  onOpen={() => setDialog({ kind: "detail", volume })}
                  onDelete={() => remove(volume)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {dialog?.kind === "create" ? (
        <CreateVolume onClose={() => setDialog(null)} onCreated={reload} />
      ) : null}
      {dialog?.kind === "detail" ? (
        <VolumeDetail volume={dialog.volume} onClose={() => setDialog(null)} />
      ) : null}
    </>
  );
};
