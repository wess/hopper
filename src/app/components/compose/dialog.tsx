// Compose up/down dialog — pick one or more compose files and bring a stack up
// or down with the full set of options (project, env-file, profiles, build,
// force-recreate, remove-orphans). Validates with `docker compose config` before
// launching, and streams output live via composeProgress.

import { openFile } from "@basket/dialog";
import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { ComposeOptions } from "../../../shared/types.ts";
import { runComposeAction } from "../../lib/composerun.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";
import { Console } from "../stacks/console.tsx";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly onClose: () => void;
  readonly onDone: () => void;
  readonly initialFiles?: readonly string[];
  readonly initialProject?: string;
};

export const ComposeDialog = ({ onClose, onDone, initialFiles, initialProject }: Props) => {
  const [files, setFiles] = useState<string[]>(
    initialFiles && initialFiles.length > 0 ? [...initialFiles] : [""],
  );
  const [project, setProject] = useState(initialProject ?? "");
  const [envFile, setEnvFile] = useState("");
  const [profiles, setProfiles] = useState("");
  const [build, setBuild] = useState(false);
  const [forceRecreate, setForceRecreate] = useState(false);
  const [removeOrphans, setRemoveOrphans] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [valid, setValid] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);

  const append = (line: string) => setLog((prev) => [...prev, line]);

  const cleanFiles = (): string[] => files.map((f) => f.trim()).filter(Boolean);

  const setFile = (i: number, value: string) =>
    setFiles((prev) => prev.map((f, idx) => (idx === i ? value : f)));
  const addFile = () => setFiles((prev) => [...prev, ""]);
  const removeFile = (i: number) =>
    setFiles((prev) => (prev.length > 1 ? prev.filter((_, idx) => idx !== i) : prev));

  const browseFile = async (i: number) => {
    const picked = await openFile({
      title: "Select a compose file",
      filters: [{ name: "Compose", extensions: ["yml", "yaml"] }],
    });
    if (picked) setFile(i, picked);
  };

  const browseEnv = async () => {
    const picked = await openFile({ title: "Select an env file" });
    if (picked) setEnvFile(picked);
  };

  const options = (): ComposeOptions => ({
    profiles: profiles
      .split(",")
      .map((p) => p.trim())
      .filter(Boolean),
    build,
    forceRecreate,
    removeOrphans,
  });

  const validate = async () => {
    const fs = cleanFiles();
    if (fs.length === 0) {
      setError("At least one compose file is required.");
      return;
    }
    setBusy(true);
    setError(null);
    setValid(null);
    try {
      const res = await invoke(ch.composeConfig, {
        files: fs,
        project: project.trim() || undefined,
      });
      if (res.ok) setValid("Configuration is valid.");
      else setError(res.error ?? "Invalid compose configuration.");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const run = async (action: "up" | "down") => {
    const fs = cleanFiles();
    if (fs.length === 0) {
      setError("At least one compose file is required.");
      return;
    }
    setBusy(true);
    setError(null);
    setValid(null);
    setLog([]);
    try {
      const res = await runComposeAction(
        action,
        { files: fs, project: project.trim() || undefined, envFile: envFile.trim() || undefined },
        action === "up" ? options() : { removeOrphans },
        (line) => append(line),
      );
      if (res.ok) {
        toast.success(action === "up" ? "Stack started" : "Stack stopped");
        onDone();
        onClose();
      } else {
        setError(res.error ?? `Compose ${action} failed.`);
        setBusy(false);
      }
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title="Compose" onClose={onClose} width={640}>
      <Field
        label="Compose files"
        hint="docker-compose.yml / compose.yaml — add overrides in order"
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {files.map((f, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are positional inputs
            <div key={i} style={{ display: "flex", gap: 8 }}>
              <Input
                value={f}
                onChange={(e) => setFile(i, e.target.value)}
                placeholder="/path/to/docker-compose.yml"
                autoFocus={i === 0}
              />
              <Button onClick={() => browseFile(i)}>Browse…</Button>
              {files.length > 1 ? (
                <Button variant="ghost" onClick={() => removeFile(i)} aria-label="Remove file">
                  ✕
                </Button>
              ) : null}
            </div>
          ))}
          <div>
            <Button size="sm" variant="ghost" onClick={addFile}>
              + Add file
            </Button>
          </div>
        </div>
      </Field>

      <div style={{ display: "flex", gap: 12 }}>
        <Field label="Project name" hint="optional — defaults to the folder name">
          <Input value={project} onChange={(e) => setProject(e.target.value)} />
        </Field>
        <Field label="Profiles" hint="comma-separated, optional">
          <Input
            value={profiles}
            onChange={(e) => setProfiles(e.target.value)}
            placeholder="dev, debug"
          />
        </Field>
      </div>

      <Field label="Env file" hint="optional --env-file">
        <div style={{ display: "flex", gap: 8 }}>
          <Input value={envFile} onChange={(e) => setEnvFile(e.target.value)} placeholder=".env" />
          <Button onClick={browseEnv}>Browse…</Button>
        </div>
      </Field>

      <div style={{ display: "flex", gap: 16, flexWrap: "wrap", marginTop: 4 }}>
        <label className="checkbox-row">
          <input type="checkbox" checked={build} onChange={(e) => setBuild(e.target.checked)} />
          Build images
        </label>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={forceRecreate}
            onChange={(e) => setForceRecreate(e.target.checked)}
          />
          Force recreate
        </label>
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={removeOrphans}
            onChange={(e) => setRemoveOrphans(e.target.checked)}
          />
          Remove orphans
        </label>
      </div>

      <Console lines={log} />

      {valid ? (
        <div style={{ color: "var(--green)", fontSize: 12.5, marginTop: 8 }}>{valid}</div>
      ) : null}
      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, marginTop: 8, whiteSpace: "pre-wrap" }}>
          {error}
        </div>
      ) : null}

      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          {busy ? "Close" : "Cancel"}
        </Button>
        <Button onClick={validate} disabled={busy}>
          Validate
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
