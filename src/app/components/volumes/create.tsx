// Create-volume dialog — name + optional driver (defaults to "local").

import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly onClose: () => void;
  readonly onCreated: () => void;
};

export const CreateVolume = ({ onClose, onCreated }: Props) => {
  const [name, setName] = useState("");
  const [driver, setDriver] = useState("local");
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
      await invoke(ch.volumeCreate, { name: name.trim(), driver: driver.trim() || "local" });
      toast.success(`Created ${name.trim()}`);
      onCreated();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title="Create volume" onClose={onClose} width={480}>
      <Field label="Name">
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="my-data"
          autoFocus
        />
      </Field>
      <Field label="Driver" hint="Defaults to local.">
        <Input value={driver} onChange={(e) => setDriver(e.target.value)} placeholder="local" />
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
