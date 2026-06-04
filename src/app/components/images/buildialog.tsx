// Build dialog — pick a context folder + Dockerfile, stream `docker build`
// output via `buildProgress` events keyed by a generated requestId, then
// resolve with the built image id.

import { openFolder } from "@basket/dialog";
import { toast } from "@basket/ui/toast";
import { useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { BuildInput, BuildProgress } from "../../../shared/types.ts";
import { errorMessage, invoke, subscribe } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly onClose: () => void;
  readonly onBuilt: () => void;
};

// Parse a textarea of `KEY=VALUE` build-arg lines into an object.
const parseArgs = (text: string): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const i = line.indexOf("=");
    if (i === -1) continue;
    out[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  return out;
};

export const BuildDialog = ({ onClose, onBuilt }: Props) => {
  const [context, setContext] = useState("");
  const [dockerfile, setDockerfile] = useState("Dockerfile");
  const [tag, setTag] = useState("");
  const [target, setTarget] = useState("");
  const [args, setArgs] = useState("");
  const [noCache, setNoCache] = useState(false);
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
    const dir = await openFolder({ title: "Select build context" });
    if (dir) setContext(dir);
  };

  const start = async () => {
    if (!context.trim()) {
      setError("A build context folder is required.");
      return;
    }
    setBusy(true);
    setError(null);
    setLog([]);
    const requestId = crypto.randomUUID();

    const unsub = subscribe(ch.buildProgress, (p: BuildProgress) => {
      if (p.requestId !== requestId) return;
      if (p.stream) {
        for (const l of p.stream.split("\n")) if (l) append(l);
      } else if (p.status && !p.done) {
        append(p.status);
      }
      if (p.error) setError(p.error);
    });

    const input: BuildInput = {
      contextDir: context.trim(),
      dockerfile: dockerfile.trim() || "Dockerfile",
      tag: tag.trim() || undefined,
      target: target.trim() || undefined,
      buildArgs: parseArgs(args),
      noCache,
    };
    try {
      const res = await invoke(ch.imageBuild, { requestId, input });
      if (res.ok) {
        toast.success(tag.trim() ? `Built ${tag.trim()}` : "Image built");
        onBuilt();
        onClose();
      } else {
        setError(res.error ?? "Build failed.");
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
    <Modal title="Build an Image" onClose={onClose} width={620}>
      <Field label="Build context" hint="the folder sent to the daemon as the build context">
        <div style={{ display: "flex", gap: 8 }}>
          <Input
            value={context}
            onChange={(e) => setContext(e.target.value)}
            placeholder="/path/to/project"
            autoFocus
          />
          <Button onClick={browse}>Browse…</Button>
        </div>
      </Field>
      <Field label="Dockerfile" hint="relative to the context folder">
        <Input value={dockerfile} onChange={(e) => setDockerfile(e.target.value)} />
      </Field>
      <Field label="Tag" hint="optional — e.g. myapp:latest, ghcr.io/owner/app:1.0">
        <Input value={tag} onChange={(e) => setTag(e.target.value)} placeholder="myapp:latest" />
      </Field>
      <Field label="Target stage" hint="optional — multi-stage build target">
        <Input value={target} onChange={(e) => setTarget(e.target.value)} placeholder="" />
      </Field>
      <Field label="Build args" hint="one KEY=VALUE per line">
        <textarea
          className="input"
          rows={2}
          value={args}
          onChange={(e) => setArgs(e.target.value)}
          placeholder={"VERSION=1.2.3"}
        />
      </Field>
      <label className="field-inline">
        <input type="checkbox" checked={noCache} onChange={(e) => setNoCache(e.target.checked)} />
        <span>No cache</span>
      </label>

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
        <Button variant="primary" onClick={start} disabled={busy}>
          {busy ? "Building…" : "Build"}
        </Button>
      </div>
    </Modal>
  );
};
