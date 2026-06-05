// Migrate from Docker Desktop — detect the source engine, pick what to bring
// over, and run the migration with live progress. Lives in Settings; safe to
// run repeatedly (the source is never modified).

import { toast } from "@basket/ui/toast";
import { useState } from "react";
import * as ch from "../../shared/channels.ts";
import type { MigrationItem, MigrationProgress, MigrationScan } from "../../shared/types.ts";
import { errorMessage, invoke, useEvent } from "../lib/ipc.ts";
import { Button } from "./ui.tsx";

type Kind = "containers" | "images" | "volumes" | "networks";

const KINDS: { readonly key: Kind; readonly label: string }[] = [
  { key: "containers", label: "Containers" },
  { key: "images", label: "Images" },
  { key: "volumes", label: "Volumes" },
  { key: "networks", label: "Networks" },
];

type Sel = Record<Kind, Set<string>>;
const allSelected = (scan: MigrationScan): Sel => ({
  containers: new Set(scan.containers.map((i) => i.id)),
  images: new Set(scan.images.map((i) => i.id)),
  volumes: new Set(scan.volumes.map((i) => i.id)),
  networks: new Set(scan.networks.map((i) => i.id)),
});

const ItemRow = ({
  item,
  checked,
  onToggle,
}: {
  item: MigrationItem;
  checked: boolean;
  onToggle: () => void;
}) => (
  <label
    style={{
      display: "flex",
      alignItems: "center",
      gap: 8,
      padding: "4px 0",
      cursor: "pointer",
      fontSize: 13,
    }}
  >
    <input type="checkbox" checked={checked} onChange={onToggle} />
    <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
      {item.name}
    </span>
    {item.detail ? <span className="stat-sub">{item.detail}</span> : null}
  </label>
);

export const MigrationPanel = () => {
  const [scan, setScan] = useState<MigrationScan | null>(null);
  const [sel, setSel] = useState<Sel | null>(null);
  const [scanning, setScanning] = useState(false);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<MigrationProgress | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);

  // Live per-item progress from the host during a migration run. Functional
  // setState keeps this closure safe regardless of when the event fires.
  useEvent(ch.migrationProgress, (p) => {
    setProgress(p);
    if (p.error) setErrors((prev) => [...prev, p.error as string]);
    if (p.warning) setWarnings((prev) => [...prev, p.warning as string]);
    if (p.finished) {
      setRunning(false);
      if (p.phase === "error") toast.error("Migration failed", { description: p.message });
      else toast.success("Migration complete", { description: "Containers recreated (stopped)." });
    }
  });

  const doScan = async () => {
    if (scanning || running) return;
    setScanning(true);
    setProgress(null);
    setErrors([]);
    setWarnings([]);
    try {
      const result = await invoke(ch.migrationScan, undefined);
      setScan(result);
      setSel(result.available ? allSelected(result) : null);
      if (!result.available) toast.info("Nothing to migrate", { description: result.message });
    } catch (e) {
      toast.error("Scan failed", { description: errorMessage(e) });
    } finally {
      setScanning(false);
    }
  };

  const toggle = (kind: Kind, id: string) =>
    setSel((prev) => {
      if (!prev) return prev;
      const next = new Set(prev[kind]);
      next.has(id) ? next.delete(id) : next.add(id);
      return { ...prev, [kind]: next };
    });

  const toggleAll = (kind: Kind) =>
    setSel((prev) => {
      if (!prev || !scan) return prev;
      const items = scan[kind];
      const full = prev[kind].size === items.length;
      return { ...prev, [kind]: full ? new Set() : new Set(items.map((i) => i.id)) };
    });

  const total = sel ? KINDS.reduce((n, k) => n + sel[k.key].size, 0) : 0;

  const run = async () => {
    if (!sel || running || total === 0) return;
    setRunning(true);
    setErrors([]);
    setWarnings([]);
    setProgress({ phase: "networks", item: "", done: 0, total: 0, message: "Starting…" });
    try {
      // Pin the source we scanned so a daemon change between scan and run can't
      // redirect the migration to a different engine.
      await invoke(ch.migrationRun, {
        source: scan?.sourceEndpoint,
        containers: [...sel.containers],
        images: [...sel.images],
        volumes: [...sel.volumes],
        networks: [...sel.networks],
      });
      // Success/failure is reported via the finished progress event (a single
      // toast there) — no toast here, to avoid a duplicate.
    } catch (e) {
      toast.error("Migration failed", { description: errorMessage(e) });
      setRunning(false);
    }
  };

  return (
    <div className="panel">
      <div className="panel-title">
        Migrate from Docker Desktop
        <Button size="sm" onClick={doScan} disabled={scanning || running}>
          {scanning ? "Scanning…" : scan ? "Rescan" : "Scan"}
        </Button>
      </div>

      {!scan ? (
        <div className="stat-sub">
          Bring containers, images, volumes, and networks over from Docker Desktop (or another
          Docker engine) into Hopper. Your existing engine is never modified.
        </div>
      ) : null}

      {scan && !scan.available ? <div className="stat-sub">{scan.message}</div> : null}

      {scan?.available && sel ? (
        <>
          <div className="stat-sub" style={{ marginBottom: 10 }}>
            Source: {scan.source}. Containers are recreated stopped — start them when you're ready.
          </div>
          {KINDS.map(({ key, label }) => {
            const items = scan[key];
            if (items.length === 0) return null;
            const chosen = sel[key].size;
            return (
              <div key={key} style={{ marginBottom: 12 }}>
                <label
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    fontWeight: 600,
                    fontSize: 13,
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={chosen === items.length}
                    ref={(el) => {
                      if (el) el.indeterminate = chosen > 0 && chosen < items.length;
                    }}
                    onChange={() => toggleAll(key)}
                  />
                  {label}
                  <span className="stat-sub" style={{ fontWeight: 400 }}>
                    {chosen}/{items.length}
                  </span>
                </label>
                <div style={{ paddingLeft: 22 }}>
                  {items.map((item) => (
                    <ItemRow
                      key={item.id}
                      item={item}
                      checked={sel[key].has(item.id)}
                      onToggle={() => toggle(key, item.id)}
                    />
                  ))}
                </div>
              </div>
            );
          })}

          <div style={{ display: "flex", alignItems: "center", gap: 12, marginTop: 6 }}>
            <Button variant="primary" onClick={run} disabled={running || total === 0}>
              {running ? "Migrating…" : `Migrate ${total} item${total === 1 ? "" : "s"}`}
            </Button>
            {progress ? (
              <span className="stat-sub">
                {progress.phase !== "done" && progress.phase !== "error"
                  ? `${progress.phase}${
                      progress.total > 0
                        ? ` ${Math.min(progress.done + 1, progress.total)}/${progress.total}`
                        : ""
                    }: ${progress.message}`
                  : progress.message}
              </span>
            ) : null}
          </div>

          {errors.length ? (
            <div className="stat-sub" style={{ marginTop: 10, color: "var(--danger, #e5534b)" }}>
              {errors.length} item{errors.length === 1 ? "" : "s"} skipped:
              <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
                {errors.slice(0, 6).map((e) => (
                  <li key={e}>{e}</li>
                ))}
              </ul>
            </div>
          ) : null}

          {warnings.length ? (
            <div className="stat-sub" style={{ marginTop: 10, color: "var(--warn, #d29922)" }}>
              {warnings.length} warning{warnings.length === 1 ? "" : "s"}:
              <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
                {warnings.slice(0, 6).map((w) => (
                  <li key={w}>{w}</li>
                ))}
              </ul>
            </div>
          ) : null}
        </>
      ) : null}
    </div>
  );
};
