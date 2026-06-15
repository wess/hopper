// Images view — list local images with search, pull, Hub search, prune, and
// per-row Run / Tag / Delete actions plus a detail modal (Inspect/History/Tags).

import { toast } from "@basket/ui/toast";
import { Download, Hammer, Play, Search, Tag, Trash2, Upload } from "lucide-react";
import { useMemo, useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { Image } from "../../shared/types.ts";
import { confirm } from "../lib/confirm.tsx";
import { ago, bytes, imageName, shortId } from "../lib/format.ts";
import { errorMessage, invoke, useEvent, useLoad } from "../lib/ipc.ts";
import { imageMatchesWorkspace } from "../lib/workspaces.ts";
import { useActiveWorkspace } from "../state/store.ts";
import { BuildDialog } from "./images/buildialog.tsx";
import { ImageDetail } from "./images/detail.tsx";
import { HubSearchDialog } from "./images/hubsearch.tsx";
import { PullDialog } from "./images/pull.tsx";
import { PushDialog } from "./images/pushdialog.tsx";
import { TagDialog } from "./images/tagdialog.tsx";
import { RunDialog } from "./rundialog.tsx";
import { Badge, Button, EmptyState, Spinner, Toolbar, ViewHeader } from "./ui.tsx";

type Dialog =
  | { readonly kind: "pull"; readonly ref?: string }
  | { readonly kind: "build" }
  | { readonly kind: "hub" }
  | { readonly kind: "run"; readonly image: string }
  | { readonly kind: "tag"; readonly image: Image }
  | { readonly kind: "push"; readonly ref: string }
  | { readonly kind: "detail"; readonly image: Image };

const ImageRow = ({
  image,
  onOpen,
  onRun,
  onTag,
  onPush,
  onDelete,
}: {
  readonly image: Image;
  readonly onOpen: () => void;
  readonly onRun: () => void;
  readonly onTag: () => void;
  readonly onPush: () => void;
  readonly onDelete: () => void;
}) => {
  const extra = image.repoTags.slice(1);
  const inUse = image.containers > 0;
  const act = (fn: () => void) => (e: React.MouseEvent) => {
    e.stopPropagation();
    fn();
  };
  return (
    <tr className="clickable" onClick={onOpen} style={{ opacity: image.dangling ? 0.6 : 1 }}>
      <td>
        <div className="cell-name">{imageName(image.repoTags, image.id)}</div>
        {extra.length > 0 ? (
          <div className="chips" style={{ marginTop: 4 }}>
            {extra.map((t) => (
              <span key={t} className="chip">
                {t}
              </span>
            ))}
          </div>
        ) : null}
      </td>
      <td className="cell-mono">{shortId(image.id)}</td>
      <td>
        <Badge tone={inUse ? "green" : "neutral"}>{inUse ? "In use" : "Unused"}</Badge>
      </td>
      <td className="right cell-sub">{bytes(image.size)}</td>
      <td className="right cell-sub" style={{ whiteSpace: "nowrap" }}>
        {ago(image.created)}
      </td>
      <td className="right">
        <div className="cell-actions">
          <button
            type="button"
            className="icon-btn"
            title="Run"
            onClick={act(onRun)}
            disabled={image.repoTags.length === 0}
          >
            <Play size={15} />
          </button>
          <button type="button" className="icon-btn" title="Tag" onClick={act(onTag)}>
            <Tag size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            title="Push"
            onClick={act(onPush)}
            disabled={image.repoTags.length === 0}
          >
            <Upload size={15} />
          </button>
          <button
            type="button"
            className="icon-btn"
            data-danger="true"
            title="Delete"
            onClick={act(onDelete)}
          >
            <Trash2 size={15} />
          </button>
        </div>
      </td>
    </tr>
  );
};

export const ImagesView = () => {
  const { data, error, loading, reload } = useLoad(ch.imageList, { all: false });
  const [query, setQuery] = useState("");
  const [dialog, setDialog] = useState<Dialog | null>(null);

  useEvent(ch.resourcesChanged, (e) => {
    if (e.kind === "image") reload();
  });

  const workspace = useActiveWorkspace();
  const images = data ?? [];
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return images.filter((i) => {
      if (!imageMatchesWorkspace(i.repoTags, workspace)) return false;
      if (!q) return true;
      return (
        imageName(i.repoTags, i.id).toLowerCase().includes(q) || i.id.toLowerCase().includes(q)
      );
    });
  }, [images, query, workspace]);

  const remove = async (image: Image) => {
    const label = imageName(image.repoTags, image.id);
    if (!(await confirm({ title: `Delete image ${label}?`, confirmLabel: "Delete", danger: true })))
      return;
    try {
      await invoke(ch.imageRemove, { id: image.id });
      toast.success(`Removed ${label}`);
      reload();
    } catch (e) {
      const msg = errorMessage(e);
      if (/in use|conflict|cannot be forced|being used/i.test(msg)) {
        if (
          await confirm({
            title: `${label} is in use.`,
            message: "Force remove?",
            confirmLabel: "Force remove",
            danger: true,
          })
        ) {
          try {
            await invoke(ch.imageRemove, { id: image.id, force: true });
            toast.success(`Force-removed ${label}`);
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
    const all = await confirm({
      title: "Remove ALL unused images?",
      message: "Prune = all unused, Cancel = dangling only.",
      confirmLabel: "Prune",
      danger: true,
    });
    try {
      const report = await invoke(ch.imagePrune, { all });
      toast.success(`Reclaimed ${bytes(report.reclaimed)}`, {
        description: `${report.removed} image(s) removed`,
      });
      reload();
    } catch (e) {
      toast.error("Prune failed", { description: errorMessage(e) });
    }
  };

  return (
    <>
      <ViewHeader title="Images" count={images.length}>
        <Button onClick={() => setDialog({ kind: "hub" })}>
          <Search size={15} /> Search Hub
        </Button>
        <Button onClick={prune}>
          <Trash2 size={15} /> Prune
        </Button>
        <Button onClick={() => setDialog({ kind: "build" })}>
          <Hammer size={15} /> Build
        </Button>
        <Button variant="primary" onClick={() => setDialog({ kind: "pull" })}>
          <Download size={15} /> Pull
        </Button>
      </ViewHeader>

      <Toolbar>
        <div className="search-input">
          <Search size={15} />
          <input
            className="input"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search images…"
          />
        </div>
      </Toolbar>

      {loading ? (
        <Spinner label="Loading images…" />
      ) : error ? (
        <EmptyState
          title="Couldn’t load images"
          hint={error}
          action={<Button onClick={reload}>Retry</Button>}
        />
      ) : filtered.length === 0 ? (
        <EmptyState
          icon={<Download size={32} />}
          title={query ? "No matching images" : "No images yet"}
          hint={query ? "Try a different search." : "Pull an image to get started."}
          action={
            query ? undefined : (
              <Button variant="primary" onClick={() => setDialog({ kind: "pull" })}>
                <Download size={15} /> Pull an image
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
                <th>Image ID</th>
                <th>Status</th>
                <th className="right">Size</th>
                <th className="right">Created</th>
                <th className="right">Actions</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((image) => (
                <ImageRow
                  key={image.id}
                  image={image}
                  onOpen={() => setDialog({ kind: "detail", image })}
                  onRun={() => setDialog({ kind: "run", image: image.repoTags[0] ?? image.id })}
                  onTag={() => setDialog({ kind: "tag", image })}
                  onPush={() => setDialog({ kind: "push", ref: image.repoTags[0] ?? image.id })}
                  onDelete={() => remove(image)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {dialog?.kind === "pull" ? (
        <PullDialog initialRef={dialog.ref} onClose={() => setDialog(null)} onPulled={reload} />
      ) : null}
      {dialog?.kind === "build" ? (
        <BuildDialog onClose={() => setDialog(null)} onBuilt={reload} />
      ) : null}
      {dialog?.kind === "push" ? (
        <PushDialog initialRef={dialog.ref} onClose={() => setDialog(null)} onPushed={reload} />
      ) : null}
      {dialog?.kind === "hub" ? (
        <HubSearchDialog
          onClose={() => setDialog(null)}
          onPull={(name) => setDialog({ kind: "pull", ref: name })}
        />
      ) : null}
      {dialog?.kind === "run" ? (
        <RunDialog image={dialog.image} onClose={() => setDialog(null)} onLaunched={reload} />
      ) : null}
      {dialog?.kind === "tag" ? (
        <TagDialog image={dialog.image} onClose={() => setDialog(null)} onTagged={reload} />
      ) : null}
      {dialog?.kind === "detail" ? (
        <ImageDetail image={dialog.image} onClose={() => setDialog(null)} onChanged={reload} />
      ) : null}
    </>
  );
};
