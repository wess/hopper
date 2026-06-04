// Live log console for a container. Starts the log stream on mount, renders
// incoming `logLine` events (matched by requestId === container id), and stops
// the stream on unmount.

import { useEffect, useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { LogLine } from "../../../shared/types.ts";
import { invoke, useEvent } from "../../lib/ipc.ts";
import { Button } from "../ui.tsx";

type Line = { readonly key: number; readonly text: string; readonly stream: string };

export const Logs = ({ id }: { id: string }) => {
  const [lines, setLines] = useState<readonly Line[]>([]);
  const seq = useRef(0);
  const bodyRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    setLines([]);
    seq.current = 0;
    invoke(ch.logStart, { id, tail: 500 }).catch(() => {});
    return () => {
      invoke(ch.logStop, { requestId: id }).catch(() => {});
    };
  }, [id]);

  useEvent<LogLine>(ch.logLine, (line) => {
    if (line.requestId !== id) return;
    setLines((prev) => {
      const next = [...prev, { key: seq.current++, text: line.text, stream: line.stream }];
      return next.length > 5000 ? next.slice(next.length - 5000) : next;
    });
  });

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on every append.
  useEffect(() => {
    const el = bodyRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const onScroll = () => {
    const el = bodyRef.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  return (
    <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div className="console-toolbar">
        <Button size="sm" variant="ghost" onClick={() => setLines([])}>
          Clear
        </Button>
        <span className="cell-sub">{lines.length} lines</span>
      </div>
      <div className="console" ref={bodyRef} onScroll={onScroll}>
        {lines.map((l) => (
          <span key={l.key} className="log-line" data-stream={l.stream}>
            {l.text}
          </span>
        ))}
      </div>
    </div>
  );
};
