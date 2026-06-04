// Push dialog — push a local image to its registry, streaming progress per
// layer via `pushProgress` events. Credentials are resolved on the host from
// the user's docker config; this dialog never handles secrets.

import { toast } from "@basket/ui/toast";
import { useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { PushProgress } from "../../../shared/types.ts";
import { errorMessage, invoke, subscribe } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly initialRef: string;
  readonly onClose: () => void;
  readonly onPushed: () => void;
};

type Layer = { readonly status: string; readonly current: number; readonly total: number };

export const PushDialog = ({ initialRef, onClose, onPushed }: Props) => {
  const [ref, setRef] = useState(initialRef);
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
    setOverall(`Pushing ${target}…`);
    const requestId = crypto.randomUUID();

    const unsub = subscribe(ch.pushProgress, (p: PushProgress) => {
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
      const res = await invoke(ch.imagePush, { requestId, ref: target });
      if (res.ok) {
        toast.success(`Pushed ${target}`);
        onPushed();
        onClose();
      } else {
        setError(res.error ?? "Push failed.");
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
    <Modal title="Push an Image" onClose={onClose} width={560}>
      <Field label="Image reference" hint="must be tagged for its target registry">
        <Input
          value={ref}
          onChange={(e) => setRef(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && !busy && start()}
          placeholder="ghcr.io/owner/app:tag"
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
          {busy ? "Pushing…" : "Push"}
        </Button>
      </div>
    </Modal>
  );
};
