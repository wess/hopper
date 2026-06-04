// "Run a container" dialog — create + start from an image with the common
// options (name, ports, env, volumes, restart policy). Shared by the Images
// view (run-from-image) and the Containers view (the "+ Run" button).

import { useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { RunInput } from "../../shared/types.ts";
import { errorMessage, invoke } from "../lib/ipc.ts";
import { Button, Field, Input, Modal } from "./ui.tsx";

type Props = {
  // Pre-fill the image when launched from an image row.
  readonly image?: string;
  readonly onClose: () => void;
  readonly onLaunched?: (id: string) => void;
};

// Parse a textarea of `KEY=VALUE` / `host:container` lines, one per row.
const lines = (text: string): string[] =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

export const RunDialog = ({ image: initialImage, onClose, onLaunched }: Props) => {
  const [image, setImage] = useState(initialImage ?? "");
  const [name, setName] = useState("");
  const [ports, setPorts] = useState("");
  const [env, setEnv] = useState("");
  const [vols, setVols] = useState("");
  const [command, setCommand] = useState("");
  const [restart, setRestart] = useState("no");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!image.trim()) {
      setError("An image is required.");
      return;
    }
    setBusy(true);
    setError(null);
    const input: RunInput = {
      image: image.trim(),
      name: name.trim() || undefined,
      command: command.trim() || undefined,
      restart: restart === "no" ? undefined : restart,
      env: lines(env),
      ports: lines(ports).map((p) => {
        const [host, rest] = p.split(":");
        const [container, proto] = (rest ?? host ?? "").split("/");
        return { host: host ?? "", container: container ?? host ?? "", proto: proto || "tcp" };
      }),
      volumes: lines(vols).map((v) => {
        const [host, container, ro] = v.split(":");
        return { host: host ?? "", container: container ?? "", ro: ro === "ro" };
      }),
    };
    try {
      const { id } = await invoke(ch.containerRun, input);
      onLaunched?.(id);
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title="Run a Container" onClose={onClose} width={560}>
      <Field label="Image" hint="e.g. nginx:latest, postgres:16, redis">
        <Input
          value={image}
          onChange={(e) => setImage(e.target.value)}
          placeholder="nginx:latest"
          autoFocus
        />
      </Field>
      <Field label="Container name" hint="optional">
        <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="my-app" />
      </Field>
      <Field label="Port mappings" hint="one per line — host:container or host:container/udp">
        <textarea
          className="input"
          rows={2}
          value={ports}
          onChange={(e) => setPorts(e.target.value)}
          placeholder={"8080:80\n5432:5432"}
        />
      </Field>
      <Field label="Environment" hint="one KEY=VALUE per line">
        <textarea
          className="input"
          rows={2}
          value={env}
          onChange={(e) => setEnv(e.target.value)}
          placeholder={"POSTGRES_PASSWORD=secret"}
        />
      </Field>
      <Field label="Volumes" hint="one per line — /host/path:/container/path[:ro]">
        <textarea
          className="input"
          rows={2}
          value={vols}
          onChange={(e) => setVols(e.target.value)}
          placeholder={"/data:/var/lib/postgresql/data"}
        />
      </Field>
      <Field label="Command override" hint="optional">
        <Input value={command} onChange={(e) => setCommand(e.target.value)} placeholder="" />
      </Field>
      <Field label="Restart policy">
        <select className="input" value={restart} onChange={(e) => setRestart(e.target.value)}>
          <option value="no">No</option>
          <option value="on-failure">On failure</option>
          <option value="always">Always</option>
          <option value="unless-stopped">Unless stopped</option>
        </select>
      </Field>

      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, marginTop: 4 }}>{error}</div>
      ) : null}

      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={submit} disabled={busy}>
          {busy ? "Starting…" : "Run"}
        </Button>
      </div>
    </Modal>
  );
};
