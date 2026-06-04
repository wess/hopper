// Compose dialog — pick a compose file and bring the stack up or down by
// driving the `docker compose` CLI on the host. Output streams live via
// `composeProgress` events keyed by a generated requestId.

import { openFile } from "@basket/dialog";
import { toast } from "@basket/ui/toast";
import { useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { ComposeProgress } from "../../../shared/types.ts";
import { errorMessage, invoke, subscribe } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly onClose: () => void;
  readonly onDone: () => void;
};

export const ComposeDialog = ({ onClose, onDone }: Props) => {
  const [file, setFile] = useState("");
  const [project, setProject] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const logRef = useRef<HTMLPreElement>(null);

  const append = (line: string) => {
    setLog((prev) => [...prev, line]);
    queueMicrotask(() => {
      if (logRef.current) logRef.current.scrollTop = logRef.current.scrollHeight;
    });
  };

  const browse = async () => {
    const picked = await openFile({
      title: "Select a compose file",
      filters: [{ name: "Compose", extensions: ["yml", "yaml"] }],
    });
    if (picked) setFile(picked);
  };

  const run = async (op: "up" | "down") => {
    if (!file.trim()) {
      setError("A compose file is required.");
      return;
    }
    setBusy(true);
    setError(null);
    setLog([]);
    const requestId = crypto.randomUUID();

    const unsub = subscribe(ch.composeProgress, (p: ComposeProgress) => {
      if (p.requestId !== requestId) return;
      if (p.line) append(p.line);
      if (p.error) setError(p.error);
    });

    const payload = { requestId, file: file.trim(), project: project.trim() || undefined };
    try {
      const res = await invoke(op === "up" ? ch.composeUp : ch.composeDown, payload);
      if (res.ok) {
        toast.success(op === "up" ? "Stack started" : "Stack stopped");
        onDone();
        onClose();
      } else {
        setError(res.error ?? `Compose ${op} failed.`);
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
    <Modal title="Compose" onClose={onClose} width={620}>
      <Field label="Compose file" hint="docker-compose.yml / compose.yaml">
        <div style={{ display: "flex", gap: 8 }}>
          <Input
            value={file}
            onChange={(e) => setFile(e.target.value)}
            placeholder="/path/to/docker-compose.yml"
            autoFocus
          />
          <Button onClick={browse}>Browse…</Button>
        </div>
      </Field>
      <Field label="Project name" hint="optional — defaults to the folder name">
        <Input value={project} onChange={(e) => setProject(e.target.value)} placeholder="" />
      </Field>

      {log.length > 0 ? (
        <pre ref={logRef} className="build-log">
          {log.join("\n")}
        </pre>
      ) : null}

      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, marginTop: 8 }}>{error}</div>
      ) : null}

      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          {busy ? "Close" : "Cancel"}
        </Button>
        <Button variant="danger" onClick={() => run("down")} disabled={busy}>
          Down
        </Button>
        <Button variant="primary" onClick={() => run("up")} disabled={busy}>
          {busy ? "Working…" : "Up"}
        </Button>
      </div>
    </Modal>
  );
};
