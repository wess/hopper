// A promise-based confirm dialog, styled to match the app (replacing native
// window.confirm). `confirm(opts)` resolves true/false; mount <ConfirmHost/>
// once near the app root. A tiny external store drives the single live request.

import { useSyncExternalStore } from "react";
import { Button, Modal } from "../components/ui.tsx";

type Req = {
  readonly title: string;
  readonly message?: string;
  readonly confirmLabel: string;
  readonly danger: boolean;
  readonly resolve: (ok: boolean) => void;
};

let current: Req | null = null;
const listeners = new Set<() => void>();
const set = (r: Req | null): void => {
  current = r;
  for (const l of listeners) l();
};

export const confirm = (opts: {
  title: string;
  message?: string;
  confirmLabel?: string;
  danger?: boolean;
}): Promise<boolean> =>
  new Promise((resolve) => {
    set({
      title: opts.title,
      message: opts.message,
      confirmLabel: opts.confirmLabel ?? "Confirm",
      danger: opts.danger ?? false,
      resolve,
    });
  });

const useCurrent = (): Req | null =>
  useSyncExternalStore(
    (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    () => current,
  );

export const ConfirmHost = () => {
  const req = useCurrent();
  if (!req) return null;
  const done = (ok: boolean): void => {
    req.resolve(ok);
    set(null);
  };
  return (
    <Modal title={req.title} onClose={() => done(false)} width={420}>
      {req.message ? <p className="confirm-message">{req.message}</p> : null}
      <div className="modal-actions">
        <Button variant="ghost" onClick={() => done(false)}>
          Cancel
        </Button>
        <Button variant={req.danger ? "danger" : "primary"} onClick={() => done(true)}>
          {req.confirmLabel}
        </Button>
      </div>
    </Modal>
  );
};
