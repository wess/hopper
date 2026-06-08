// Registry sign-in: authenticate to Docker Hub or any registry from inside the
// app (no `docker login` CLI). Credentials land in the OS keychain and are used
// by push/pull automatically.

import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import { errorMessage, invoke, useLoad } from "../../lib/ipc.ts";
import { Button, Field, Input } from "../ui.tsx";

export const RegistryAccountsPanel = () => {
  const { data: connections, reload } = useLoad(ch.registryConnections, undefined, []);
  const [server, setServer] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  const signIn = async () => {
    if (busy || !username || !password) return;
    setBusy(true);
    try {
      const res = await invoke(ch.registryLogin, { server, username, password });
      if (res.ok) {
        toast.success(`Signed in to ${server.trim() || "Docker Hub"}`);
        setServer("");
        setUsername("");
        setPassword("");
        reload();
      } else {
        toast.error("Sign-in failed", { description: res.error });
      }
    } catch (e) {
      toast.error("Sign-in failed", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  const signOut = (target: string) => async () => {
    try {
      await invoke(ch.registryLogout, { server: target });
      reload();
    } catch (e) {
      toast.error("Couldn't sign out", { description: errorMessage(e) });
    }
  };

  return (
    <div className="panel">
      <div className="panel-title">Registries</div>
      <div className="stat-sub" style={{ marginBottom: 12 }}>
        Sign in to push and pull private images — and lift Docker Hub's anonymous pull rate limit —
        without the docker CLI.
      </div>

      {connections && connections.length > 0 ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 6, marginBottom: 14 }}>
          {connections.map((c) => (
            <div
              key={c.server}
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 10,
              }}
            >
              <div style={{ minWidth: 0 }}>
                <div style={{ fontWeight: 600 }}>{c.label}</div>
                <div className="stat-sub">
                  {c.username ? `Signed in as ${c.username}` : "From your docker config"}
                </div>
              </div>
              {c.source === "hopper" ? (
                <Button size="sm" variant="ghost" onClick={signOut(c.server)}>
                  Sign out
                </Button>
              ) : (
                <span className="stat-sub">read-only</span>
              )}
            </div>
          ))}
        </div>
      ) : null}

      <Field label="Registry" hint="leave blank for Docker Hub">
        <Input placeholder="docker.io" value={server} onChange={(e) => setServer(e.target.value)} />
      </Field>
      <Field label="Username">
        <Input value={username} onChange={(e) => setUsername(e.target.value)} autoComplete="off" />
      </Field>
      <Field label="Password or access token">
        <Input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          autoComplete="off"
        />
      </Field>
      <div style={{ marginTop: 8 }}>
        <Button variant="primary" onClick={signIn} disabled={busy || !username || !password}>
          {busy ? "Signing in…" : "Sign in"}
        </Button>
      </div>
    </div>
  );
};
