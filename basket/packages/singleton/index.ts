import { on } from "butter";

export type SecondInstanceInfo = {
  readonly argv: readonly string[];
  readonly cwd: string;
};

type Handler = (info: SecondInstanceInfo) => void | Promise<void>;

const handlers: Handler[] = [];
let wired = false;

const ensureWired = (): void => {
  if (wired) return;
  wired = true;
  on("app:secondinstance", (data: unknown) => {
    const raw = (data ?? {}) as { argv?: unknown; cwd?: unknown };
    const info: SecondInstanceInfo = {
      argv: Array.isArray(raw.argv) ? (raw.argv as string[]) : [],
      cwd: typeof raw.cwd === "string" ? raw.cwd : "",
    };
    void Promise.all(
      handlers.map((h) =>
        Promise.resolve()
          .then(() => h(info))
          .catch(() => {}),
      ),
    );
  });
};

export const onSecondInstance = (handler: Handler): (() => void) => {
  ensureWired();
  handlers.push(handler);
  return () => {
    const i = handlers.indexOf(handler);
    if (i >= 0) handlers.splice(i, 1);
  };
};

// By the time host code runs, the singleinstance plugin has already
// decided we're the leader (or the process has exited). True here is
// always correct unless the user disabled the plugin in butter.yaml.
export const isLeader = (): boolean => true;
