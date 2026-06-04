import { on } from "butter";

type Handler = () => void | Promise<void>;

const beforeQuit: Handler[] = [];
let beforeQuitWired = false;
const ensureBeforeQuit = (): void => {
  if (beforeQuitWired) return;
  beforeQuitWired = true;
  on("app:beforequit", () => {
    void Promise.all(
      beforeQuit.map((h) =>
        Promise.resolve()
          .then(h)
          .catch(() => {}),
      ),
    );
  });
  process.on("beforeExit", () => {
    for (const h of beforeQuit) {
      try {
        const r = h();
        if (r instanceof Promise) void r.catch(() => {});
      } catch {
        // swallow
      }
    }
  });
};

const willQuit: Handler[] = [];
let willQuitWired = false;
const ensureWillQuit = (): void => {
  if (willQuitWired) return;
  willQuitWired = true;
  on("app:willquit", () => {
    void Promise.all(
      willQuit.map((h) =>
        Promise.resolve()
          .then(h)
          .catch(() => {}),
      ),
    );
  });
};

const activate: Handler[] = [];
let activateWired = false;
const ensureActivate = (): void => {
  if (activateWired) return;
  activateWired = true;
  on("app:activate", () => {
    void Promise.all(
      activate.map((h) =>
        Promise.resolve()
          .then(h)
          .catch(() => {}),
      ),
    );
  });
};

const reopen: Handler[] = [];
let reopenWired = false;
const ensureReopen = (): void => {
  if (reopenWired) return;
  reopenWired = true;
  on("app:reopen", () => {
    void Promise.all(
      reopen.map((h) =>
        Promise.resolve()
          .then(h)
          .catch(() => {}),
      ),
    );
  });
};

const subscribe = (list: Handler[], handler: Handler, ensure: () => void): (() => void) => {
  ensure();
  list.push(handler);
  return () => {
    const i = list.indexOf(handler);
    if (i >= 0) list.splice(i, 1);
  };
};

export const onBeforeQuit = (handler: Handler) => subscribe(beforeQuit, handler, ensureBeforeQuit);

export const onWillQuit = (handler: Handler) => subscribe(willQuit, handler, ensureWillQuit);

export const onActivate = (handler: Handler) => subscribe(activate, handler, ensureActivate);

export const onReopen = (handler: Handler) => subscribe(reopen, handler, ensureReopen);
