import { useEffect, useState } from "react";

export type ToastVariant = "info" | "success" | "error";

export type ToastInput = {
  readonly title: string;
  readonly description?: string;
  readonly variant?: ToastVariant;
  readonly duration?: number;
};

type Internal = ToastInput & { readonly id: string };

type Listener = (toasts: readonly Internal[]) => void;

let toasts: Internal[] = [];
const listeners = new Set<Listener>();

const notify = () => {
  const snapshot = [...toasts];
  for (const fn of listeners) fn(snapshot);
};

const push = (t: ToastInput, variant: ToastVariant): string => {
  const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const entry: Internal = { ...t, variant: t.variant ?? variant, id };
  toasts = [...toasts, entry];
  notify();
  const duration = t.duration ?? 4000;
  if (duration > 0) {
    setTimeout(() => {
      toasts = toasts.filter((x) => x.id !== id);
      notify();
    }, duration);
  }
  return id;
};

type ToastFn = ((title: string, options?: Omit<ToastInput, "title">) => string) & {
  success: (title: string, options?: Omit<ToastInput, "title">) => string;
  error: (title: string, options?: Omit<ToastInput, "title">) => string;
  info: (title: string, options?: Omit<ToastInput, "title">) => string;
  dismiss: (id: string) => void;
};

const baseFn = ((title: string, options?: Omit<ToastInput, "title">) =>
  push({ title, ...(options ?? {}) }, "info")) as ToastFn;

baseFn.success = (title, options) => push({ title, ...(options ?? {}) }, "success");
baseFn.error = (title, options) => push({ title, ...(options ?? {}) }, "error");
baseFn.info = (title, options) => push({ title, ...(options ?? {}) }, "info");
baseFn.dismiss = (id) => {
  toasts = toasts.filter((x) => x.id !== id);
  notify();
};

export const toast: ToastFn = baseFn;

export const Toaster = () => {
  const [list, setList] = useState<readonly Internal[]>(toasts);
  useEffect(() => {
    const fn: Listener = (next) => setList(next);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);

  return (
    <div className="basket-toaster">
      {list.map((t) => (
        <div key={t.id} className="basket-toast" data-variant={t.variant}>
          <div className="basket-toast-title">{t.title}</div>
          {t.description ? <div className="basket-toast-description">{t.description}</div> : null}
        </div>
      ))}
    </div>
  );
};
