// Interactive shell for a running container via the exec stream. Opens
// `/bin/sh` on mount, pipes `execChunk` output into a console, and forwards
// keystrokes through `execInput`. Stops the session on unmount.

import { useEffect, useRef, useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { ExecChunk } from "../../../shared/types.ts";
import { errorMessage, invoke, useEvent } from "../../lib/ipc.ts";

export const Terminal = ({ id }: { id: string }) => {
  const [out, setOut] = useState("");
  const [done, setDone] = useState(false);
  const sessionId = useRef<string | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    let live = true;
    setOut("");
    setDone(false);
    inputRef.current?.focus();
    invoke(ch.execStart, { id, cmd: ["/bin/sh"] })
      .then((r) => {
        if (live) sessionId.current = r.sessionId;
        else invoke(ch.execStop, { sessionId: r.sessionId }).catch(() => {});
      })
      .catch((e) => {
        if (live) setOut(`failed to start shell: ${errorMessage(e)}\n`);
      });
    return () => {
      live = false;
      const sid = sessionId.current;
      sessionId.current = null;
      if (sid) invoke(ch.execStop, { sessionId: sid }).catch(() => {});
    };
  }, [id]);

  useEvent<ExecChunk>(ch.execChunk, (chunk) => {
    if (chunk.sessionId !== sessionId.current) return;
    if (chunk.text) setOut((prev) => prev + chunk.text);
    if (chunk.error) setOut((prev) => `${prev}\n[error] ${chunk.error}\n`);
    if (chunk.done) setDone(true);
  });

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll on output.
  useEffect(() => {
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [out]);

  const send = (data: string) => {
    const sid = sessionId.current;
    if (sid) invoke(ch.execInput, { sessionId: sid, data }).catch(() => {});
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (done) return;
    if (e.key === "Enter") {
      e.preventDefault();
      const el = inputRef.current;
      const value = el?.value ?? "";
      send(`${value}\n`);
      if (el) el.value = "";
    } else if (e.key === "Tab") {
      e.preventDefault();
      send("\t");
    } else if (e.ctrlKey && e.key.length === 1) {
      e.preventDefault();
      const code = e.key.toUpperCase().charCodeAt(0) - 64;
      if (code > 0 && code < 27) send(String.fromCharCode(code));
    }
  };

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: clicking the pane focuses the shell input.
    // biome-ignore lint/a11y/noStaticElementInteractions: focus shim for the hidden input.
    <div
      style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}
      onClick={() => inputRef.current?.focus()}
    >
      <div className="console" ref={bodyRef}>
        {out}
        {done ? <span className="cell-sub">[session closed]</span> : null}
      </div>
      <textarea
        ref={inputRef}
        className="input"
        rows={1}
        spellCheck={false}
        placeholder={done ? "session closed" : "type a command, Enter to send"}
        disabled={done}
        onKeyDown={onKeyDown}
        style={{ borderRadius: 0, fontFamily: "var(--mono)", fontSize: 12 }}
      />
    </div>
  );
};
