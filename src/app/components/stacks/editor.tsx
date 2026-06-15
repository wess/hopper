// Compose file viewer/editor. Loads a file from the stack's config set, lets
// you edit it, validate the whole set with `docker compose config`, and save.

import { toast } from "@basket/ui/toast";
import { useEffect, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";
import { Button, EmptyState, Spinner } from "../ui.tsx";

export const Editor = ({ project, files }: { project: string; files: readonly string[] }) => {
  const [index, setIndex] = useState(0);
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  const path = files[index];

  useEffect(() => {
    if (!path) return;
    let live = true;
    setLoading(true);
    setError(null);
    setOk(null);
    invoke(ch.composeReadFile, { path })
      .then((res) => {
        if (!live) return;
        if (res.ok) setContent(res.content ?? "");
        else setError(res.error ?? "Could not read file.");
      })
      .catch((e) => live && setError(errorMessage(e)))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [path]);

  if (files.length === 0) {
    return (
      <EmptyState
        title="No compose file recorded"
        hint="This stack's containers don't carry a config-file label, so the source file can't be located."
      />
    );
  }

  const validate = async () => {
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const res = await invoke(ch.composeConfig, { files: [...files], project });
      if (res.ok) setOk("Configuration is valid.");
      else setError(res.error ?? "Invalid compose configuration.");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!path) return;
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const res = await invoke(ch.composeWriteFile, { path, content });
      if (res.ok) toast.success("Saved", { description: path });
      else setError(res.error ?? "Could not save file.");
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, minHeight: 0, flex: 1 }}>
      <div className="toolbar" style={{ padding: 0 }}>
        {files.length > 1 ? (
          <select
            className="input"
            value={index}
            onChange={(e) => setIndex(Number(e.target.value))}
            style={{ maxWidth: 360 }}
          >
            {files.map((f, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: files are positional
              <option key={i} value={i}>
                {f}
              </option>
            ))}
          </select>
        ) : (
          <span className="cell-sub" style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
            {path}
          </span>
        )}
        <div className="toolbar-spacer" />
        <Button size="sm" onClick={validate} disabled={busy || loading}>
          Validate
        </Button>
        <Button size="sm" variant="primary" onClick={save} disabled={busy || loading}>
          Save
        </Button>
      </div>

      {loading ? (
        <Spinner label="Loading file…" />
      ) : (
        <textarea
          className="input code-editor"
          spellCheck={false}
          value={content}
          onChange={(e) => setContent(e.target.value)}
          style={{ flex: 1, minHeight: 240, resize: "none", fontFamily: "var(--mono)" }}
        />
      )}

      {ok ? <div style={{ color: "var(--green)", fontSize: 12.5 }}>{ok}</div> : null}
      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, whiteSpace: "pre-wrap" }}>{error}</div>
      ) : null}
    </div>
  );
};
