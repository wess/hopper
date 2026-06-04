// Headline stat cards for the dashboard overview — containers / images /
// volumes / networks counts, sourced from systemInfo plus the list channels.

import * as ch from "../../../shared/channels.ts";
import { useEvent, useLoad } from "../../lib/ipc.ts";

type Card = {
  readonly label: string;
  readonly value: string;
  readonly sub?: string;
  readonly subGreen?: boolean;
};

export const Stats = () => {
  const info = useLoad(ch.systemInfo, undefined, []);
  const images = useLoad(ch.imageList, { all: false }, []);
  const volumes = useLoad(ch.volumeList, undefined, []);
  const networks = useLoad(ch.networkList, undefined, []);

  useEvent(ch.resourcesChanged, (e) => {
    info.reload();
    if (e.kind === "image") images.reload();
    if (e.kind === "volume") volumes.reload();
    if (e.kind === "network") networks.reload();
  });

  const running = info.data?.containersRunning ?? 0;
  const total = info.data?.containers ?? 0;

  const cards: readonly Card[] = [
    {
      label: "Containers",
      value: String(running),
      sub: `${running} running / ${total} total`,
      subGreen: running > 0,
    },
    { label: "Images", value: String(images.data?.length ?? info.data?.images ?? 0) },
    { label: "Volumes", value: String(volumes.data?.length ?? 0) },
    { label: "Networks", value: String(networks.data?.length ?? 0) },
  ];

  return (
    <div className="stat-grid">
      {cards.map((c) => (
        <div className="stat-card" key={c.label}>
          <div className="stat-label">{c.label}</div>
          <div className="stat-value" style={c.subGreen ? { color: "var(--green)" } : undefined}>
            {c.value}
          </div>
          {c.sub ? <div className="stat-sub">{c.sub}</div> : null}
        </div>
      ))}
    </div>
  );
};
