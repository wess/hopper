// Connect-a-container control for a network's detail modal. Lists currently
// connected containers (parsed from the inspect `Containers` map) each with a
// Disconnect button, plus a dropdown of running containers to Connect.

import { toast } from "@basket/ui/toast";
import { X } from "lucide-react";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { InspectResult } from "../../../shared/types.ts";
import { shortId } from "../../lib/format.ts";
import { errorMessage, invoke, useLoad } from "../../lib/ipc.ts";
import { Button } from "../ui.tsx";

type Connected = { readonly id: string; readonly name: string };

const parseConnected = (inspect: InspectResult | null): Connected[] => {
  const map = inspect?.Containers;
  if (!map || typeof map !== "object") return [];
  return Object.entries(map as Record<string, unknown>).map(([id, info]) => {
    const name =
      info && typeof info === "object" && typeof (info as { Name?: unknown }).Name === "string"
        ? (info as { Name: string }).Name
        : shortId(id);
    return { id, name };
  });
};

type Props = {
  readonly networkId: string;
  readonly inspect: InspectResult | null;
  readonly onChanged: () => void;
};

export const ConnectPanel = ({ networkId, inspect, onChanged }: Props) => {
  const { data: containers } = useLoad(ch.containerList, { all: false });
  const [selected, setSelected] = useState("");
  const [busy, setBusy] = useState(false);

  const connected = parseConnected(inspect);
  const connectedIds = new Set(connected.map((c) => c.id));
  const available = (containers ?? []).filter((c) => !connectedIds.has(c.id));

  const connect = async () => {
    if (!selected) return;
    setBusy(true);
    try {
      await invoke(ch.networkConnect, { id: networkId, container: selected });
      toast.success("Container connected");
      setSelected("");
      onChanged();
    } catch (e) {
      toast.error("Connect failed", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async (container: string, label: string) => {
    setBusy(true);
    try {
      await invoke(ch.networkDisconnect, { id: networkId, container });
      toast.success(`Disconnected ${label}`);
      onChanged();
    } catch (e) {
      toast.error("Disconnect failed", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <div className="section-title">Connected containers</div>
      {connected.length === 0 ? (
        <div className="cell-sub">No containers connected.</div>
      ) : (
        <div className="chips">
          {connected.map((c) => (
            <span
              key={c.id}
              className="chip"
              style={{ display: "inline-flex", alignItems: "center", gap: 6 }}
            >
              {c.name}
              <button
                type="button"
                className="icon-btn"
                style={{ width: 18, height: 18 }}
                title="Disconnect"
                disabled={busy}
                onClick={() => disconnect(c.id, c.name)}
              >
                <X size={12} />
              </button>
            </span>
          ))}
        </div>
      )}

      <div className="section-title">Connect a container</div>
      <div style={{ display: "flex", gap: 8 }}>
        <select
          className="input"
          value={selected}
          onChange={(e) => setSelected(e.target.value)}
          style={{ flex: 1 }}
        >
          <option value="">Select a running container…</option>
          {available.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
        <Button variant="primary" onClick={connect} disabled={busy || !selected}>
          Connect
        </Button>
      </div>
    </div>
  );
};
