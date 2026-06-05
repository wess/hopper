// First-run / disconnected experience. Shown in the content area whenever the
// engine isn't connected. For a managed provider (our VM, native dockerd) it's
// a one-click "Start engine"; otherwise it explains what's wrong and offers a
// retry. This is the moment Hopper stops feeling like it *needs* Docker Desktop.

import { toast } from "@basket/ui/toast";
import { Container, Play, RefreshCw, Settings as SettingsIcon } from "lucide-react";
import { useState } from "react";
import * as ch from "../../shared/channels.ts";
import { errorMessage, invoke } from "../lib/ipc.ts";
import { setState, setView, useApp } from "../state/store.ts";
import { Button } from "./ui.tsx";

export const OnboardingView = () => {
  const { engine } = useApp();
  const [busy, setBusy] = useState(false);
  // Reflect either our local in-flight action or the host's reported "starting".
  const starting = busy || engine.state === "starting";

  const run = (channel: typeof ch.engineStart | typeof ch.enginePing) => async () => {
    if (busy) return;
    setBusy(true);
    try {
      setState({ engine: await invoke(channel, undefined) });
    } catch (e) {
      toast.error("Couldn't reach the engine", { description: errorMessage(e) });
    } finally {
      setBusy(false);
    }
  };

  const title = engine.managed ? "Start the Hopper engine" : "Connect a Docker engine";
  const body = engine.managed
    ? "Hopper runs its own Docker engine in a lightweight Linux VM on Apple Virtualization — no Docker Desktop required."
    : "Hopper couldn't reach a running Docker engine. Start one, or point Hopper at it with DOCKER_HOST.";

  return (
    <div className="onboarding">
      <div className="onboarding-card">
        <div className="onboarding-icon">
          <Container size={40} strokeWidth={1.5} />
        </div>
        <h1 className="onboarding-title">{title}</h1>
        <p className="onboarding-body">{body}</p>

        <div className="onboarding-status" data-state={engine.state}>
          <span className="engine-dot" />
          <span>{engine.message}</span>
          {engine.detail ? <span className="onboarding-detail">{engine.detail}</span> : null}
        </div>

        <div className="onboarding-actions">
          {engine.managed ? (
            <Button variant="primary" onClick={run(ch.engineStart)} disabled={starting}>
              <Play size={15} />
              {starting ? "Starting…" : "Start engine"}
            </Button>
          ) : null}
          <Button onClick={run(ch.enginePing)} disabled={busy}>
            <RefreshCw size={15} />
            Retry
          </Button>
          <Button variant="ghost" onClick={() => setView("settings")}>
            <SettingsIcon size={15} />
            Settings
          </Button>
        </div>

        {engine.provider ? (
          <div className="onboarding-provider">Provider: {engine.provider}</div>
        ) : null}
      </div>
    </div>
  );
};
