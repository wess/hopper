// Live activity feed — accumulates the docker event firehose, newest first,
// capped so the panel stays light.

import { Activity } from "lucide-react";
import { useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { DockerEvent } from "../../../shared/types.ts";
import { ago } from "../../lib/format.ts";
import { useEvent } from "../../lib/ipc.ts";
import { EmptyState } from "../ui.tsx";

const CAP = 50;

type Entry = DockerEvent & { readonly seq: number };

export const Feed = () => {
  const [events, setEvents] = useState<readonly Entry[]>([]);
  const seq = useRef(0);

  useEvent(ch.dockerEvent, (e) => {
    seq.current += 1;
    const entry: Entry = { ...e, seq: seq.current };
    setEvents((prev) => [entry, ...prev].slice(0, CAP));
  });

  return (
    <div className="panel" style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
      <div className="panel-title">Activity</div>
      {events.length === 0 ? (
        <EmptyState
          icon={<Activity size={28} />}
          title="No activity yet"
          hint="Docker events appear here as containers, images and volumes change."
        />
      ) : (
        <div className="feed" style={{ overflow: "auto" }}>
          {events.map((e) => (
            <div className="feed-row" key={e.seq}>
              <span className="feed-type">{e.type}</span>
              <span className="feed-action">{e.action}</span>
              <span className="feed-actor">{e.actor}</span>
              <span className="feed-time">{ago(e.time)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};
