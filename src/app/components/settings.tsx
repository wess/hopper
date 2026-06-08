// Settings — engine connection, theme, workspaces, the MCP server snippet,
// and about/version details.

import { toast } from "@basket/ui/toast";
import { useEffect, useMemo, useState } from "react";
import { version } from "../../../package.json";
import * as ch from "../../shared/channels.ts";
import type { Workspace } from "../../shared/types.ts";
import { bytes } from "../lib/format.ts";
import { errorMessage, invoke, useLoad } from "../lib/ipc.ts";
import {
  deleteWorkspace,
  saveWorkspace,
  setState,
  setTheme,
  type ThemePref,
  useApp,
} from "../state/store.ts";
import { GithubAccountPanel, RegistryAccountsPanel } from "./accounts/index.tsx";
import { MigrationPanel } from "./migrate.tsx";
import { Button, Field, Input, Modal, ViewHeader } from "./ui.tsx";

type EngineBusy = "" | "testing" | "starting" | "stopping" | "reclaiming";

const EnginePanel = () => {
  const { engine } = useApp();
  const [busy, setBusy] = useState<EngineBusy>("");
  // Live VM stats for managed engines; refetched when connectivity flips.
  const { data: stats, reload: reloadStats } = useLoad(ch.engineStats, undefined, [
    engine.connected,
    engine.provider,
  ]);

  const test = async () => {
    if (busy) return;
    setBusy("testing");
    try {
      const status = await invoke(ch.enginePing, undefined);
      setState({ engine: status });
      if (status.connected) toast.success("Engine reachable", { description: status.message });
      else toast.error("Engine unreachable", { description: status.detail ?? status.message });
    } catch (e) {
      toast.error("Connection test failed", { description: errorMessage(e) });
    } finally {
      setBusy("");
    }
  };

  // Managed providers (the Hopper VM, native dockerd) can be brought up/down
  // from here. The host broadcasts the resulting status; we also apply the
  // returned value immediately for responsiveness.
  const lifecycle = (action: "starting" | "stopping") => async () => {
    if (busy) return;
    setBusy(action);
    try {
      const status = await invoke(
        action === "starting" ? ch.engineStart : ch.engineStop,
        undefined,
      );
      setState({ engine: status });
    } catch (e) {
      toast.error(`Couldn't ${action === "starting" ? "start" : "stop"} the engine`, {
        description: errorMessage(e),
      });
    } finally {
      setBusy("");
    }
  };

  // Reclaim unused VM disk space (fstrim in the guest).
  const reclaim = async () => {
    if (busy) return;
    setBusy("reclaiming");
    try {
      const result = await invoke(ch.engineReclaim, undefined);
      if (result.ok) {
        toast.success("Disk reclaimed", { description: result.detail });
        reloadStats();
      } else {
        toast.error("Reclaim unavailable", { description: result.detail });
      }
    } catch (e) {
      toast.error("Reclaim failed", { description: errorMessage(e) });
    } finally {
      setBusy("");
    }
  };

  return (
    <div className="panel">
      <div className="panel-title">
        Engine
        <div style={{ display: "flex", gap: 8 }}>
          {engine.managed && !engine.connected ? (
            <Button
              size="sm"
              variant="primary"
              onClick={lifecycle("starting")}
              disabled={busy !== ""}
            >
              {busy === "starting" ? "Starting…" : "Start engine"}
            </Button>
          ) : null}
          {engine.managed && engine.connected ? (
            <Button
              size="sm"
              variant="ghost"
              onClick={lifecycle("stopping")}
              disabled={busy !== ""}
            >
              {busy === "stopping" ? "Stopping…" : "Stop engine"}
            </Button>
          ) : null}
          {engine.managed && engine.connected ? (
            <Button size="sm" onClick={reclaim} disabled={busy !== ""}>
              {busy === "reclaiming" ? "Reclaiming…" : "Reclaim disk"}
            </Button>
          ) : null}
          <Button size="sm" onClick={test} disabled={busy !== ""}>
            {busy === "testing" ? "Testing…" : "Test connection"}
          </Button>
        </div>
      </div>
      <div
        className="engine-status"
        data-connected={engine.connected}
        style={{ padding: "0 0 12px" }}
      >
        <span className="engine-dot" />
        <span>{engine.message}</span>
      </div>
      {engine.detail ? (
        <div className="stat-sub" style={{ marginBottom: 8 }}>
          {engine.detail}
        </div>
      ) : null}
      <dl className="kv">
        <dt>Provider</dt>
        <dd>{engine.provider || "—"}</dd>
        <dt>Endpoint</dt>
        <dd>{engine.endpoint ?? "—"}</dd>
        {engine.managed && stats ? (
          <>
            <dt>VM memory</dt>
            <dd>
              {bytes((stats.memTotalKb - stats.memAvailKb) * 1024)} /{" "}
              {bytes(stats.memTotalKb * 1024)}
            </dd>
            <dt>VM disk</dt>
            <dd>
              {bytes(stats.diskUsedKb * 1024)} / {bytes(stats.diskTotalKb * 1024)}
            </dd>
            <dt>Load</dt>
            <dd>{stats.load1}</dd>
          </>
        ) : null}
      </dl>
      <div className="stat-sub" style={{ marginTop: 8 }}>
        Override the target with the <code>DOCKER_HOST</code> or <code>DOCKER_SOCKET</code>{" "}
        environment variable.
      </div>
    </div>
  );
};

const THEMES: readonly ThemePref[] = ["light", "dark", "system"];

const ThemePanel = () => {
  const { theme } = useApp();
  return (
    <div className="panel">
      <div className="panel-title">Appearance</div>
      <div className="segmented">
        {THEMES.map((t) => (
          <button
            type="button"
            key={t}
            className="segmented-opt"
            data-active={theme === t}
            onClick={() => setTheme(t)}
          >
            {t[0]?.toUpperCase()}
            {t.slice(1)}
          </button>
        ))}
      </div>
      <div className="stat-sub" style={{ marginTop: 10 }}>
        <strong>System</strong> follows your OS appearance. Cycle anytime with <strong>⌘⇧L</strong>.
      </div>
    </div>
  );
};

const summarize = (w: Workspace): string => {
  const parts: string[] = [];
  if (w.composeProjects?.length) parts.push(`${w.composeProjects.length} project(s)`);
  if (w.namePattern?.trim()) parts.push(`/${w.namePattern}/`);
  return parts.length ? parts.join(" · ") : "Matches everything";
};

const WorkspaceEditor = ({
  initial,
  projects,
  onClose,
}: {
  initial: Workspace | null;
  projects: readonly string[];
  onClose: () => void;
}) => {
  const [name, setName] = useState(initial?.name ?? "");
  const [selected, setSelected] = useState<string[]>([...(initial?.composeProjects ?? [])]);
  const [pattern, setPattern] = useState(initial?.namePattern ?? "");

  const toggle = (p: string) =>
    setSelected((s) => (s.includes(p) ? s.filter((x) => x !== p) : [...s, p]));

  const save = () => {
    if (!name.trim()) {
      toast.error("Name your workspace");
      return;
    }
    const id = initial?.id ?? crypto.randomUUID?.() ?? `ws-${Date.now()}`;
    saveWorkspace({
      id,
      name: name.trim(),
      composeProjects: selected.length ? selected : undefined,
      namePattern: pattern.trim() || undefined,
    });
    toast.success(initial ? "Workspace updated" : "Workspace created");
    onClose();
  };

  return (
    <Modal title={initial ? "Edit Workspace" : "New Workspace"} onClose={onClose} width={500}>
      <Field label="Name">
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Backend"
          autoFocus
        />
      </Field>
      <Field label="Compose projects" hint="containers in any selected project are in scope">
        <div className="ws-projects">
          {projects.length === 0 ? (
            <span className="stat-sub">No compose projects detected.</span>
          ) : (
            projects.map((p) => (
              <button
                type="button"
                key={p}
                className="ws-chip"
                data-on={selected.includes(p)}
                onClick={() => toggle(p)}
              >
                {p}
              </button>
            ))
          )}
        </div>
      </Field>
      <Field label="Name pattern" hint="optional regex matched against container/image names">
        <Input
          value={pattern}
          onChange={(e) => setPattern(e.target.value)}
          placeholder="^web|api"
        />
      </Field>
      <div className="modal-actions">
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" onClick={save}>
          {initial ? "Save" : "Create"}
        </Button>
      </div>
    </Modal>
  );
};

const WorkspacesPanel = () => {
  const { workspaces } = useApp();
  const { data: containers } = useLoad(ch.containerList, { all: true }, []);
  const [editing, setEditing] = useState<Workspace | null | "closed">("closed");

  const projects = useMemo(() => {
    const set = new Set<string>();
    for (const c of containers ?? []) if (c.composeProject) set.add(c.composeProject);
    return [...set].sort();
  }, [containers]);

  return (
    <div className="panel">
      <div className="panel-title">
        Workspaces
        <Button size="sm" onClick={() => setEditing(null)}>
          New workspace
        </Button>
      </div>
      <div className="stat-sub" style={{ marginBottom: 12 }}>
        A workspace scopes the Containers and Images views to the compose projects and name pattern
        you care about. Switch the active one from the sidebar.
      </div>
      {workspaces.length === 0 ? (
        <div className="stat-sub">No workspaces yet.</div>
      ) : (
        <div className="ws-list">
          {workspaces.map((w) => (
            <div className="ws-row" key={w.id}>
              <div>
                <div className="ws-name">{w.name}</div>
                <div className="ws-summary">{summarize(w)}</div>
              </div>
              <div className="ws-row-actions">
                <Button size="sm" variant="ghost" onClick={() => setEditing(w)}>
                  Edit
                </Button>
                <Button size="sm" variant="danger" onClick={() => deleteWorkspace(w.id)}>
                  Delete
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}
      {editing !== "closed" ? (
        <WorkspaceEditor
          initial={editing}
          projects={projects}
          onClose={() => setEditing("closed")}
        />
      ) : null}
    </div>
  );
};

const McpPanel = () => {
  const { data } = useLoad(ch.mcpConfig, undefined, []);
  const snippet = useMemo(() => {
    if (!data) return "";
    return JSON.stringify(
      { mcpServers: { hopper: { command: data.command, args: data.args } } },
      null,
      2,
    );
  }, [data]);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(snippet);
      toast.success("Copied MCP config");
    } catch {
      toast.error("Couldn't copy");
    }
  };

  return (
    <div className="panel">
      <div className="panel-title">
        MCP Server
        <Button size="sm" onClick={copy} disabled={!snippet}>
          Copy config
        </Button>
      </div>
      <div className="stat-sub" style={{ marginBottom: 10 }}>
        Hopper ships a Model Context Protocol server that lets AI clients (Claude Desktop, Claude
        Code, Cursor) manage Docker. Add this to your client's MCP config:
      </div>
      <pre className="code-block">{snippet || "…"}</pre>
    </div>
  );
};

const AboutPanel = () => {
  const { data, loading } = useLoad(ch.systemVersion, undefined, []);
  return (
    <div className="panel">
      <div className="panel-title">About</div>
      <div style={{ fontWeight: 650, fontSize: 16 }}>Hopper</div>
      <div className="stat-sub" style={{ marginBottom: 12 }}>
        A native Docker management app built with Butter.
      </div>
      <dl className="kv">
        <dt>App version</dt>
        <dd>{version}</dd>
        <dt>Engine version</dt>
        <dd>{loading ? "…" : (data?.version ?? "—")}</dd>
        <dt>API version</dt>
        <dd>{data?.apiVersion ?? "—"}</dd>
      </dl>
    </div>
  );
};

// VM resources for a managed engine (the Hopper VM). CPU/memory apply on
// engine restart; disk size applies to a freshly created data disk.
const ResourcesPanel = () => {
  const { engine } = useApp();
  const { data: res, reload } = useLoad(ch.engineResources, undefined, []);
  const [cpus, setCpus] = useState<number | null>(null);
  const [mem, setMem] = useState<number | null>(null);
  const [disk, setDisk] = useState<number | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (res) {
      setCpus(res.cpus);
      setMem(res.memoryGiB);
      setDisk(res.diskGiB);
    }
  }, [res]);

  // Only meaningful for a managed engine (our VM) — an external engine's
  // resources aren't ours to set.
  if (!engine.managed) return null;

  const save = async () => {
    if (saving || cpus === null || mem === null || disk === null) return;
    setSaving(true);
    try {
      const saved = await invoke(ch.engineSetResources, {
        cpus,
        memoryGiB: mem,
        diskGiB: disk,
      });
      setCpus(saved.cpus);
      setMem(saved.memoryGiB);
      setDisk(saved.diskGiB);
      toast.success("Resources saved", {
        description: "Restart the engine (Stop → Start) to apply CPU/memory changes.",
      });
      reload();
    } catch (e) {
      toast.error("Couldn't save resources", { description: errorMessage(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="panel">
      <div className="panel-title">
        Resources
        <Button size="sm" variant="primary" onClick={save} disabled={saving || cpus === null}>
          {saving ? "Saving…" : "Save"}
        </Button>
      </div>
      <Field label="CPUs">
        <Input
          type="number"
          min={1}
          value={cpus ?? ""}
          onChange={(e) => setCpus(Number(e.target.value))}
        />
      </Field>
      <Field label="Memory (GiB)">
        <Input
          type="number"
          min={1}
          value={mem ?? ""}
          onChange={(e) => setMem(Number(e.target.value))}
        />
      </Field>
      <Field label="Disk (GiB)" hint="applies to a freshly created data disk">
        <Input
          type="number"
          min={8}
          value={disk ?? ""}
          onChange={(e) => setDisk(Number(e.target.value))}
        />
      </Field>
      <div className="stat-sub" style={{ marginTop: 8 }}>
        CPU and memory changes take effect when you stop and start the engine. Idle memory is
        returned to the host automatically.
      </div>
    </div>
  );
};

export const SettingsView = () => (
  <div className="scroll-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
    <ViewHeader title="Settings" />
    <div style={{ padding: "0 22px", display: "flex", flexDirection: "column", gap: 14 }}>
      <EnginePanel />
      <ResourcesPanel />
      <RegistryAccountsPanel />
      <GithubAccountPanel />
      <MigrationPanel />
      <ThemePanel />
      <WorkspacesPanel />
      <McpPanel />
      <AboutPanel />
    </div>
  </div>
);
