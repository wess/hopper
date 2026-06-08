// GitHub connection: store a personal access token to raise registry-search
// rate limits, surface private repos, and pull private GHCR images.

import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import { errorMessage, invoke, useLoad } from "../../lib/ipc.ts";
import { Button, Field, Input } from "../ui.tsx";

const TOKEN_HELP =
  "Create a token with read:packages (for GHCR pulls) and repo (for private repo search).";

export const GithubAccountPanel = () => {
  const { data: status, reload } = useLoad(ch.githubStatus, undefined, []);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  const connect = async () => {
    if (busy || !token) return;
    setBusy(true);
    try {
      const res = await invoke(ch.githubConnect, { token });
      if (res.ok) {
        toast.success(res.login ? `Connected as ${res.login}` : "GitHub connected");
        setToken("");
        reload();
      } else {
        toast.error("Couldn't connect GitHub", { description: res.error });
      }
    } catch (e) {
      toast.error("Couldn't connect GitHub", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    try {
      await invoke(ch.githubDisconnect, undefined);
      reload();
    } catch (e) {
      toast.error("Couldn't disconnect", { description: errorMessage(e) });
    }
  };

  return (
    <div className="panel">
      <div className="panel-title">GitHub</div>
      {status?.connected ? (
        <>
          <div className="engine-status" data-connected="true" style={{ marginBottom: 8 }}>
            <span className="engine-dot" />
            <span>Connected{status.login ? ` as ${status.login}` : ""}</span>
          </div>
          <div className="stat-sub" style={{ marginBottom: 12 }}>
            GHCR pulls are authenticated and registry search is no longer rate-limited.
          </div>
          <Button size="sm" variant="ghost" onClick={disconnect}>
            Disconnect
          </Button>
        </>
      ) : (
        <>
          <div className="stat-sub" style={{ marginBottom: 12 }}>
            Connect a personal access token to pull private GHCR images and avoid search rate
            limits.
          </div>
          <Field label="Personal access token" hint={TOKEN_HELP}>
            <Input
              type="password"
              placeholder="ghp_…"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              autoComplete="off"
            />
          </Field>
          <div style={{ marginTop: 8 }}>
            <Button variant="primary" onClick={connect} disabled={busy || !token}>
              {busy ? "Connecting…" : "Connect"}
            </Button>
          </div>
        </>
      )}
    </div>
  );
};
