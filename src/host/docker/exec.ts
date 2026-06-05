// Interactive exec sessions — a real shell inside a running container.
//
// The Engine API's exec-start hijacks the HTTP connection into a raw
// bidirectional TTY stream, which `fetch` can't model. So we open the daemon
// connection directly with `Bun.connect` (unix socket, Windows pipe, or TCP —
// whatever `endpoint.ts` resolved), write the HTTP request by hand, strip the
// response headers, and treat the rest of the connection as the TTY.

import type { Socket } from "bun";
import { json } from "./client.ts";
import { connectOptions, currentEndpoint, hostHeader } from "./endpoint.ts";

const API = "v1.43";

export type ExecSession = {
  readonly id: string;
  write: (data: string) => void;
  resize: (cols: number, rows: number) => Promise<void>;
  close: () => void;
};

type Handlers = {
  onData: (text: string) => void;
  onClose: (err?: string) => void;
};

// Create the exec instance and hijack its TTY stream.
export const open = async (
  containerId: string,
  cmd: string[],
  handlers: Handlers,
): Promise<ExecSession> => {
  const created = await json<{ Id: string }>(`/containers/${containerId}/exec`, {
    method: "POST",
    body: {
      AttachStdin: true,
      AttachStdout: true,
      AttachStderr: true,
      Tty: true,
      Cmd: cmd,
    },
  });
  const execId = created.Id;

  const decoder = new TextDecoder();
  let headersDone = false;
  let headerBuf = "";

  const socketHandlers = {
    data(_sock: Socket<undefined>, chunk: Buffer) {
      if (headersDone) {
        handlers.onData(decoder.decode(chunk, { stream: true }));
        return;
      }
      // Accumulate until the blank line that ends the response headers,
      // then forward whatever body bytes came in the same packet.
      headerBuf += decoder.decode(chunk, { stream: true });
      const idx = headerBuf.indexOf("\r\n\r\n");
      if (idx !== -1) {
        headersDone = true;
        const rest = headerBuf.slice(idx + 4);
        if (rest) handlers.onData(rest);
        headerBuf = "";
      }
    },
    close() {
      handlers.onClose();
    },
    error(_sock: Socket<undefined>, err: Error) {
      handlers.onClose(String(err));
    },
  };

  // Resolve the endpoint at session open (it can change at runtime).
  const endpoint = currentEndpoint();
  // Branch rather than spread so the unix/tcp option shapes stay discriminated.
  const conn = connectOptions(endpoint);
  const socket: Socket<undefined> =
    "unix" in conn
      ? await Bun.connect({ unix: conn.unix, socket: socketHandlers })
      : await Bun.connect({
          hostname: conn.hostname,
          port: conn.port,
          tls: conn.tls,
          socket: socketHandlers,
        });

  // Hand-rolled hijack request. The blank line ends the headers; the body is
  // the exec-start payload.
  const body = JSON.stringify({ Detach: false, Tty: true });
  const request =
    `POST /${API}/exec/${execId}/start HTTP/1.1\r\n` +
    `Host: ${hostHeader(endpoint)}\r\n` +
    `Content-Type: application/json\r\n` +
    `Connection: Upgrade\r\n` +
    `Upgrade: tcp\r\n` +
    `Content-Length: ${Buffer.byteLength(body)}\r\n` +
    `\r\n${body}`;
  socket.write(request);

  return {
    id: execId,
    write: (data: string) => {
      try {
        socket.write(data);
      } catch {
        // socket gone — close will fire
      }
    },
    resize: async (cols: number, rows: number) => {
      try {
        await json<void>(`/exec/${execId}/resize`, { method: "POST", query: { h: rows, w: cols } });
      } catch {
        // resize is best-effort
      }
    },
    close: () => {
      try {
        socket.end();
      } catch {
        // already closed
      }
    },
  };
};
