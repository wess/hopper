// Pull dialog — streams `docker pull` progress per layer via `pullProgress`
// events keyed by a generated requestId, then resolves on completion.

import { toast } from "@basket/ui/toast";
import { useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { PullProgress } from "../../../shared/types.ts";
import { errorMessage, invoke, subscribe } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly initialRef?: string;
  readonly onClose: () => void;
  readonly onPulled: () => void;
};

type Layer = { readonly status: string; readonly current: number; readonly total: number };

export const PullDialog = ({ initialRef, onClose, onPulled }: Props) => {
  const [ref, setRef] = useState(initialRef ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [overall, setOverall] = useState<string | null>(null);
  const [layers, setLayers] = useState<Record<string, Layer>>({});
  const order = useRef<string[]>([]);

  const start = async () => {
    const target = ref.trim();
    if (!target) {
      setError("An image reference is required.");
      return;
    }
    setBusy(true);
    setError(null);
    setLayers({});
    order.current = [];
    setOverall(`Pulling ${target}…`);
    const requestId = crypto.randomUUID();

    const unsub = subscribe(ch.pullProgress, (p: PullProgress) => {
      if (p.requestId !== requestId) return;
      if (p.error) {
        setError(p.error);
        return;
      }
      if (p.id) {
        if (!order.current.includes(p.id)) order.current = [...order.current, p.id];
        setLayers((prev) => ({
          ...prev,
          [p.id as string]: { status: p.status, current: p.current ?? 0, total: p.total ?? 0 },
        }));
      } else {
        setOverall(p.status);
      }
    });

    try {
      const res = await invoke(ch.imagePull, { requestId, ref: target });
      if (res.ok) {
        toast.success(`Pulled ${target}`);
        onPulled();
        onClose();
      } else {
        setError(res.error ?? "Pull failed.");
        setBusy(false);
      }
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    } finally {
      unsub();
    }
  };

  return (
    <Modal title="Pull an Image" onClose={onClose} width={560}>
      <Field label="Image reference" hint="e.g. nginx:latest, postgres:16, ghcr.io/owner/app:tag">
        <Input
          value={ref}
          onChange={(e) => setRef(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !busy && start()}
          placeholder="nginx:latest"
          autoFocus
        />
      </Field>

      {overall ? <div className="section-title">{overall}</div> : null}

      {order.current.length > 0 ? (
        <div style={{ marginTop: 4 }}>
          {order.current.map((id) => {
            const l = layers[id];
            if (!l) return null;
            const pct = l.total > 0 ? Math.min(100, (l.current / l.total) * 100) : 0;
            return (
              <div key={id} className="pull-row">
                <span className="pull-id">{id}</span>
                <span className="pull-status">{l.status}</span>
                <span className="pull-bar">
                  <span className="pull-bar-fill" style={{ width: `${pct}%` }} />
                </span>
              </div>
            );
          })}
        </div>
      ) : null}

      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, marginTop: 8 }}>{error}</div>
      ) : null}

      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          {busy ? "Close" : "Cancel"}
        </Button>
        <Button variant="primary" onClick={start} disabled={busy}>
          {busy ? "Pulling…" : "Pull"}
        </Button>
      </div>
    </Modal>
  );
};
