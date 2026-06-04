// Image detail modal — Inspect / History / Tags tabs for a single image.

import { toast } from "@basket/ui/toast";
import { X } from "lucide-react";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { Image } from "../../../shared/types.ts";
import { ago, bytes, imageName, shortId } from "../../lib/format.ts";
import { errorMessage, invoke, useLoad } from "../../lib/ipc.ts";
import { Inspect } from "../inspect.tsx";
import { Modal, Spinner } from "../ui.tsx";

type Tab = "inspect" | "history" | "tags";

const InspectTab = ({ id }: { id: string }) => {
  const { data, error, loading } = useLoad(ch.imageInspect, { id }, [id]);
  if (loading) return <Spinner label="Loading…" />;
  if (error) return <div style={{ color: "var(--red)" }}>{error}</div>;
  return data ? <Inspect data={data} /> : null;
};

const HistoryTab = ({ id }: { id: string }) => {
  const { data, error, loading } = useLoad(ch.imageHistory, { id }, [id]);
  if (loading) return <Spinner label="Loading…" />;
  if (error) return <div style={{ color: "var(--red)" }}>{error}</div>;
  if (!data || data.length === 0) return <div className="cell-sub">No history.</div>;
  return (
    <table className="table">
      <thead>
        <tr>
          <th>Created</th>
          <th>Created by</th>
          <th className="right">Size</th>
        </tr>
      </thead>
      <tbody>
        {data.map((h, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: layers share ids (<missing>)
          <tr key={`${h.id}-${i}`}>
            <td className="cell-sub" style={{ whiteSpace: "nowrap" }}>
              {ago(h.created)}
            </td>
            <td
              className="cell-mono"
              style={{
                maxWidth: 360,
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
              title={h.createdBy}
            >
              {h.createdBy || "—"}
            </td>
            <td className="right cell-sub">{bytes(h.size)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
};

const TagsTab = ({ image, onChanged }: { image: Image; onChanged: () => void }) => {
  const untag = async (repoTag: string) => {
    try {
      await invoke(ch.imageRemove, { id: repoTag });
      toast.success(`Untagged ${repoTag}`);
      onChanged();
    } catch (e) {
      toast.error("Untag failed", { description: errorMessage(e) });
    }
  };
  if (image.repoTags.length === 0) return <div className="cell-sub">No tags (dangling image).</div>;
  return (
    <div className="chips">
      {image.repoTags.map((t) => (
        <span
          key={t}
          className="chip"
          style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
        >
          {t}
          <button
            type="button"
            className="icon-btn"
            style={{ width: 18, height: 18 }}
            title="Untag"
            onClick={() => untag(t)}
          >
            <X size={12} />
          </button>
        </span>
      ))}
    </div>
  );
};

export const ImageDetail = ({
  image,
  onClose,
  onChanged,
}: {
  readonly image: Image;
  readonly onClose: () => void;
  readonly onChanged: () => void;
}) => {
  const [tab, setTab] = useState<Tab>("inspect");
  return (
    <Modal title={imageName(image.repoTags, image.id)} onClose={onClose} width={680}>
      <div className="detail-title-sub" style={{ marginTop: -6, marginBottom: 10 }}>
        {shortId(image.id)} · {bytes(image.size)} · created {ago(image.created)}
      </div>
      <div className="tabs" style={{ margin: "0 -18px 14px" }}>
        {(["inspect", "history", "tags"] as const).map((t) => (
          <button
            key={t}
            type="button"
            className="tab"
            data-active={tab === t}
            onClick={() => setTab(t)}
          >
            {t.charAt(0).toUpperCase() + t.slice(1)}
          </button>
        ))}
      </div>
      {tab === "inspect" ? <InspectTab id={image.id} /> : null}
      {tab === "history" ? <HistoryTab id={image.id} /> : null}
      {tab === "tags" ? <TagsTab image={image} onChanged={onChanged} /> : null}
    </Modal>
  );
};
