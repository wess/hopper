// Volume detail modal — a key/value summary plus a raw Inspect tab.

import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { Volume } from "../../../shared/types.ts";
import { agoIso, bytes } from "../../lib/format.ts";
import { useLoad } from "../../lib/ipc.ts";
import { Inspect } from "../inspect.tsx";
import { Modal, Spinner } from "../ui.tsx";

type Tab = "summary" | "inspect";

const InspectTab = ({ name }: { name: string }) => {
  const { data, error, loading } = useLoad(ch.volumeInspect, { name }, [name]);
  if (loading) return <Spinner label="Loading…" />;
  if (error) return <div style={{ color: "var(--red)" }}>{error}</div>;
  return data ? <Inspect data={data} /> : null;
};

const Summary = ({ volume }: { volume: Volume }) => {
  const labels = Object.entries(volume.labels ?? {});
  return (
    <dl className="kv">
      <dt>Name</dt>
      <dd>{volume.name}</dd>
      <dt>Driver</dt>
      <dd>{volume.driver}</dd>
      <dt>Mountpoint</dt>
      <dd>{volume.mountpoint}</dd>
      <dt>Scope</dt>
      <dd>{volume.scope}</dd>
      <dt>Size</dt>
      <dd>{bytes(volume.size)}</dd>
      <dt>Created</dt>
      <dd>{agoIso(volume.created)}</dd>
      <dt>Labels</dt>
      <dd>
        {labels.length === 0 ? (
          "—"
        ) : (
          <div className="chips">
            {labels.map(([k, v]) => (
              <span key={k} className="chip">
                {k}={v}
              </span>
            ))}
          </div>
        )}
      </dd>
    </dl>
  );
};

export const VolumeDetail = ({
  volume,
  onClose,
}: {
  readonly volume: Volume;
  readonly onClose: () => void;
}) => {
  const [tab, setTab] = useState<Tab>("summary");
  return (
    <Modal title={volume.name} onClose={onClose} width={680}>
      <div className="tabs" style={{ margin: "0 -18px 14px" }}>
        {(["summary", "inspect"] as const).map((t) => (
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
      {tab === "summary" ? <Summary volume={volume} /> : null}
      {tab === "inspect" ? <InspectTab name={volume.name} /> : null}
    </Modal>
  );
};
