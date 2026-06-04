// Network detail modal — summary + Inspect + a connect/disconnect panel.

import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { Network } from "../../../shared/types.ts";
import { agoIso } from "../../lib/format.ts";
import { useLoad } from "../../lib/ipc.ts";
import { Inspect } from "../inspect.tsx";
import { Badge, Modal, Spinner } from "../ui.tsx";
import { ConnectPanel } from "./connect.tsx";

type Tab = "summary" | "connect" | "inspect";

const Summary = ({ network }: { network: Network }) => {
  const labels = Object.entries(network.labels ?? {});
  return (
    <dl className="kv">
      <dt>Name</dt>
      <dd>{network.name}</dd>
      <dt>ID</dt>
      <dd>{network.id}</dd>
      <dt>Driver</dt>
      <dd>{network.driver}</dd>
      <dt>Scope</dt>
      <dd>{network.scope}</dd>
      <dt>Internal</dt>
      <dd>{network.internal ? "yes" : "no"}</dd>
      <dt>Attachable</dt>
      <dd>{network.attachable ? "yes" : "no"}</dd>
      <dt>Subnet</dt>
      <dd>{network.ipam[0]?.subnet ?? "—"}</dd>
      <dt>Gateway</dt>
      <dd>{network.ipam[0]?.gateway ?? "—"}</dd>
      <dt>Created</dt>
      <dd>{agoIso(network.created)}</dd>
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

export const NetworkDetail = ({
  network,
  onClose,
  onChanged,
}: {
  readonly network: Network;
  readonly onClose: () => void;
  readonly onChanged: () => void;
}) => {
  const [tab, setTab] = useState<Tab>("summary");
  const { data, error, loading, reload } = useLoad(ch.networkInspect, { id: network.id }, [
    network.id,
  ]);

  const refresh = () => {
    reload();
    onChanged();
  };

  return (
    <Modal title={network.name} onClose={onClose} width={680}>
      <div className="detail-title-sub" style={{ marginTop: -6, marginBottom: 10 }}>
        {network.driver} · {network.scope}
        {network.internal ? <Badge tone="amber">Internal</Badge> : null}
      </div>
      <div className="tabs" style={{ margin: "0 -18px 14px" }}>
        {(["summary", "connect", "inspect"] as const).map((t) => (
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

      {tab === "summary" ? <Summary network={network} /> : null}
      {tab === "connect" ? (
        <ConnectPanel networkId={network.id} inspect={data} onChanged={refresh} />
      ) : null}
      {tab === "inspect" ? (
        loading ? (
          <Spinner label="Loading…" />
        ) : error ? (
          <div style={{ color: "var(--red)" }}>{error}</div>
        ) : data ? (
          <Inspect data={data} />
        ) : null
      ) : null}
    </Modal>
  );
};
