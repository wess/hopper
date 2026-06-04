// Tag dialog — apply a new repo:tag to an existing image via `imageTag`.

import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { Image } from "../../../shared/types.ts";
import { imageName } from "../../lib/format.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";
import { Button, Field, Input, Modal } from "../ui.tsx";

type Props = {
  readonly image: Image;
  readonly onClose: () => void;
  readonly onTagged: () => void;
};

export const TagDialog = ({ image, onClose, onTagged }: Props) => {
  const [repo, setRepo] = useState("");
  const [tag, setTag] = useState("latest");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!repo.trim()) {
      setError("A repository is required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await invoke(ch.imageTag, { id: image.id, repo: repo.trim(), tag: tag.trim() || "latest" });
      toast.success(`Tagged ${repo.trim()}:${tag.trim() || "latest"}`);
      onTagged();
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  };

  return (
    <Modal title={`Tag ${imageName(image.repoTags, image.id)}`} onClose={onClose} width={480}>
      <Field label="Repository" hint="e.g. myregistry.io/team/app">
        <Input
          value={repo}
          onChange={(e) => setRepo(e.target.value)}
          placeholder="owner/app"
          autoFocus
        />
      </Field>
      <Field label="Tag">
        <Input value={tag} onChange={(e) => setTag(e.target.value)} placeholder="latest" />
      </Field>

      {error ? (
        <div style={{ color: "var(--red)", fontSize: 12.5, marginTop: 4 }}>{error}</div>
      ) : null}

      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={submit} disabled={busy}>
          {busy ? "Tagging…" : "Add tag"}
        </Button>
      </div>
    </Modal>
  );
};
