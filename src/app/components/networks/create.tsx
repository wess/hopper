// Create-network dialog — name, driver, internal/attachable, optional subnet
// and gateway.

import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly onClose: () => void;
  readonly onCreated: () => void;
};

export const CreateNetwork = ({ onClose, onCreated }: Props) => {
  const [name, setName] = useState("");
  const [driver, setDriver] = useState("bridge");
  const [internal, setInternal] = useState(false);
  const [attachable, setAttachable] = useState(false);
  const [subnet, setSubnet] = useState("");
  const [gateway, setGateway] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!name.trim()) {
      setError("A name is required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke(ch.networkCreate, {
        name: name.trim(),
        driver: driver.trim() || "bridge",
        internal,
        attachable,
        subnet: subnet.trim() || undefined,
        gateway: gateway.trim() || undefined,
      });
      toast.success(`Created ${name.trim()}`);
      onCreated();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title="Create network" onClose={onClose} width={480}>
      <Field label="Name">
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-net"
          autoFocus
        />
      </Field>
      <Field label="Driver" hint="Defaults to bridge.">
        <Input value={driver} onChange={(e) => setDriver(e.target.value)} placeholder="bridge" />
      </Field>
      <label
        className="field"
        style={{ flexDirection: "row", alignItems: "center", gap: 8, marginBottom: 10 }}
      >
        <input type="checkbox" checked={internal} onChange={(e) => setInternal(e.target.checked)} />
        <span className="field-label">Internal (no external access)</span>
      </label>
      <label
        className="field"
        style={{ flexDirection: "row", alignItems: "center", gap: 8, marginBottom: 14 }}
      >
        <input
          type="checkbox"
          checked={attachable}
          onChange={(e) => setAttachable(e.target.checked)}
        />
        <span className="field-label">Attachable (manual container attach)</span>
      </label>
      <Field label="Subnet" hint="Optional, e.g. 172.20.0.0/16">
        <Input value={subnet} onChange={(e) => setSubnet(e.target.value)} placeholder="" />
      </Field>
      <Field label="Gateway" hint="Optional, e.g. 172.20.0.1">
        <Input value={gateway} onChange={(e) => setGateway(e.target.value)} placeholder="" />
      </Field>

      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, marginTop: 4 }}>{error}</div>
      ) : null}

      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={submit} disabled={busy}>
          {busy ? "Creating…" : "Create"}
        </Button>
      </div>
    </Modal>
  );
};
