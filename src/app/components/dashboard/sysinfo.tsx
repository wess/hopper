// System info panel — engine/API versions, host details, and resources, from
// `system:info` + `system:version`. Read-only key/value grid.

import * as ch from "../../../shared/channels.ts";
import { bytes } from "../../lib/format.ts";
import { useEvent, useLoad } from "../../lib/ipc.ts";
import { Spinner } from "../ui.tsx";

export const SysInfo = () => {
  const info = useLoad(ch.systemInfo, undefined, []);
  const version = useLoad(ch.systemVersion, undefined, []);

  useEvent(ch.resourcesChanged, () => info.reload());

  const i = info.data;
  const v = version.data;
  const loading = (info.loading && !i) || (version.loading && !v);

  const rows: readonly (readonly [string, string])[] = [
    ["Engine version", v?.version ?? i?.serverVersion ?? "—"],
    ["API version", v?.apiVersion ?? "—"],
    ["OS / Arch", i ? `${i.operatingSystem} · ${i.architecture}` : "—"],
    ["Kernel", v?.kernelVersion ?? "—"],
    ["CPUs", i ? String(i.ncpu) : "—"],
    ["Total memory", i ? bytes(i.memTotal) : "—"],
    ["Docker root dir", i?.dockerRootDir ?? "—"],
    ["Server name", i?.name ?? "—"],
  ];

  return (
    <div className="panel">
      <div className="panel-title">System</div>
      {loading ? (
        <Spinner label="Loading system info…" />
      ) : !i && !v ? (
        <div className="stat-sub">System info unavailable.</div>
      ) : (
        <dl className="kv">
          {rows.map(([k, val]) => (
            <div key={k} style={{ display: "contents" }}>
              <dt>{k}</dt>
              <dd>{val}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
};
