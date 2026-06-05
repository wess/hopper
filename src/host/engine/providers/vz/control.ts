// Talk to the hopperd helper over its line-delimited JSON control protocol.
//
// Commands are single-flight (serialized through a promise chain) so the next
// reply line maps to the command that asked for it; any reply arriving with no
// command outstanding is an unsolicited event (e.g. the guest stopped on its
// own). Output is captured line by line off the helper's stdout.

export type HopperdReply = {
  readonly ok: boolean;
  readonly state: string; // running | starting | stopped | error
  readonly detail: string;
  readonly socket?: string | null;
  readonly data?: string | null; // raw JSON from the guest agent (reclaim/stats)
};

export type HopperdCommand = "start" | "stop" | "status" | "ping" | "reclaim" | "stats";

export type Control = {
  readonly send: (cmd: HopperdCommand, timeoutMs?: number) => Promise<HopperdReply>;
  readonly onEvent: (fn: (reply: HopperdReply) => void) => void;
  // Fired once when the helper process exits (crash or clean). Used by the
  // provider to drive supervised restarts.
  readonly onExit: (fn: () => void) => void;
  readonly shutdown: () => Promise<void>;
  readonly alive: () => boolean;
};

// Accumulate bytes and yield complete lines. Pure — unit-tested.
export const lineSplitter = (): ((chunk: string) => string[]) => {
  let buf = "";
  return (chunk: string): string[] => {
    buf += chunk;
    const lines: string[] = [];
    let nl = buf.indexOf("\n");
    while (nl !== -1) {
      lines.push(buf.slice(0, nl));
      buf = buf.slice(nl + 1);
      nl = buf.indexOf("\n");
    }
    return lines;
  };
};

export const spawnHopperd = (binPath: string, env: Record<string, string> = {}): Control => {
  const proc = Bun.spawn([binPath], {
    stdin: "pipe",
    stdout: "pipe",
    stderr: "inherit",
    env: { ...process.env, ...env },
  });

  let running = true;
  let pending: ((reply: HopperdReply) => void) | null = null;
  let chain: Promise<unknown> = Promise.resolve();
  const listeners = new Set<(reply: HopperdReply) => void>();
  const exitListeners = new Set<() => void>();

  void proc.exited.then(() => {
    running = false;
    for (const fn of exitListeners) fn();
  });

  // Read stdout, splitting into JSON lines and routing each to the waiting
  // command or, if none, to event listeners.
  const pump = async (): Promise<void> => {
    const reader = proc.stdout.getReader();
    const decoder = new TextDecoder();
    const split = lineSplitter();
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      for (const line of split(decoder.decode(value, { stream: true }))) {
        const text = line.trim();
        if (!text) continue;
        let reply: HopperdReply;
        try {
          reply = JSON.parse(text) as HopperdReply;
        } catch {
          continue;
        }
        if (pending) {
          const resolve = pending;
          pending = null;
          resolve(reply);
        } else {
          for (const fn of listeners) fn(reply);
        }
      }
    }
  };
  void pump();

  const writer = proc.stdin;

  const once = (cmd: HopperdCommand, timeoutMs: number): Promise<HopperdReply> =>
    new Promise<HopperdReply>((resolve) => {
      const timer = setTimeout(() => {
        if (pending) {
          pending = null;
          resolve({ ok: false, state: "error", detail: `timed out waiting for "${cmd}"` });
        }
      }, timeoutMs);
      pending = (reply) => {
        clearTimeout(timer);
        resolve(reply);
      };
      writer.write(`${JSON.stringify({ cmd })}\n`);
      writer.flush();
    });

  const send = (cmd: HopperdCommand, timeoutMs = 8000): Promise<HopperdReply> => {
    if (!running) {
      return Promise.resolve({ ok: false, state: "stopped", detail: "helper not running" });
    }
    const next = chain.then(() => once(cmd, timeoutMs));
    chain = next.catch(() => undefined);
    return next;
  };

  return {
    send,
    onEvent: (fn) => {
      listeners.add(fn);
    },
    onExit: (fn) => {
      exitListeners.add(fn);
    },
    alive: () => running,
    shutdown: async () => {
      if (running) {
        await send("stop", 15000).catch(() => undefined);
      }
      try {
        proc.kill();
      } catch {
        // already gone
      }
      await proc.exited.catch(() => undefined);
    },
  };
};
