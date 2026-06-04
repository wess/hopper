// Disk usage panel — `docker system df` rows with reclaimable meters, a total,
// and a "Clean up" flow that runs `system:prune` and toasts each report. The
// native menu's Clean/Prune item dispatches a `hopper:prune` window event which
// triggers the same flow.

import { toast } from "@basket/ui/toast";
import { useEffect, useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { DiskUsage } from "../../../shared/types.ts";
import { bytes } from "../../lib/format.ts";
import { errorMessage, invoke, useEvent, useLoad } from "../../lib/ipc.ts";
import { Button, Spinner } from "../ui.tsx";

type Bucket = { readonly count: number; readonly size: number; readonly reclaimable: number };

const ROWS: readonly { readonly key: keyof DiskUsage; readonly label: string }[] = [
  { key: "images", label: "Images" },
  { key: "containers", label: "Containers" },
  { key: "volumes", label: "Volumes" },
  { key: "buildCache", label: "Build Cache" },
];

const tone = (ratio: number): "green" | "amber" | "red" =>
  ratio >= 0.66 ? "red" : ratio >= 0.33 ? "amber" : "green";

const Row = ({ label, b }: { label: string; b: Bucket }) => {
  const ratio = b.size > 0 ? Math.min(1, b.reclaimable / b.size) : 0;
  return (
    <div style={{ marginBottom: 12 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ fontWeight: 550 }}>{label}</span>
        <span className="stat-sub">
          {b.count} item{b.count === 1 ? "" : "s"}
        </span>
        <span className="stat-sub" style={{ marginLeft: "auto" }}>
          {bytes(b.size)}
        </span>
        <span className="stat-sub" style={{ minWidth: 110, textAlign: "right" }}>
          {bytes(b.reclaimable)} reclaimable
        </span>
      </div>
      <div className="meter">
        <div className="meter-fill" data-tone={tone(ratio)} style={{ width: `${ratio * 100}%` }} />
      </div>
    </div>
  );
};

export const Disk = () => {
  const { data, loading, error, reload } = useLoad(ch.systemDf, undefined, []);
  const [cleaning, setCleaning] = useState(false);

  useEvent(ch.resourcesChanged, () => reload());

  const cleanUp = async () => {
    if (cleaning) return;
    if (!window.confirm("Remove unused containers, networks, images and build cache?")) return;
    setCleaning(true);
    try {
      const reports = await invoke(ch.systemPrune, undefined);
      const total = reports.reduce((n, r) => n + r.reclaimed, 0);
      const lines = reports
        .filter((r) => r.removed > 0 || r.reclaimed > 0)
        .map((r) => `${r.kind}: ${r.removed} removed, ${bytes(r.reclaimed)}`);
      toast.success(`Reclaimed ${bytes(total)}`, {
        description: lines.length ? lines.join("\n") : "Nothing to clean up",
      });
      reload();
    } catch (e) {
      toast.error("Clean up failed", { description: errorMessage(e) });
    } finally {
      setCleaning(false);
    }
  };

  // The native menu's Clean/Prune item dispatches `hopper:prune`; route it to
  // the latest cleanUp via a ref so the listener stays mounted once.
  const cleanUpRef = useRef(cleanUp);
  cleanUpRef.current = cleanUp;
  useEffect(() => {
    const onPrune = () => void cleanUpRef.current();
    window.addEventListener("hopper:prune", onPrune);
    return () => window.removeEventListener("hopper:prune", onPrune);
  }, []);

  const total = data ? ROWS.reduce((n, r) => n + data[r.key].reclaimable, 0) : 0;

  return (
    <div className="panel">
      <div className="panel-title">
        Disk usage
        <Button size="sm" onClick={cleanUp} disabled={cleaning || !data}>
          {cleaning ? "Cleaning…" : "Clean up"}
        </Button>
      </div>
      {loading && !data ? (
        <Spinner label="Reading disk usage…" />
      ) : error && !data ? (
        <div className="stat-sub">Disk usage unavailable — {error}</div>
      ) : data ? (
        <>
          {ROWS.map((r) => (
            <Row key={r.key} label={r.label} b={data[r.key]} />
          ))}
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              borderTop: "1px solid var(--border)",
              paddingTop: 10,
              fontWeight: 600,
            }}
          >
            <span>Reclaimable</span>
            <span>{bytes(total)}</span>
          </div>
        </>
      ) : null}
    </div>
  );
};
