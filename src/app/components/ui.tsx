// Shared UI primitives. Every view composes these so the app stays visually
// consistent — basket ships no button styles by design, so these are ours.

import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode } from "react";
import { useEffect } from "react";

export type ButtonVariant = "default" | "primary" | "danger" | "ghost";

export const Button = ({
  variant = "default",
  size = "md",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: "sm" | "md";
}) => (
  <button type="button" className="btn" data-variant={variant} data-size={size} {...rest}>
    {children}
  </button>
);

// A toolbar row above a list/detail — left content, right content.
export const Toolbar = ({ children }: { children: ReactNode }) => (
  <div className="toolbar">{children}</div>
);
Toolbar.Spacer = () => <div className="toolbar-spacer" />;

// Colored status dot for container/resource states.
export const StatusDot = ({ state }: { state: string }) => (
  <span className="status-dot" data-state={state} title={state} />
);

export const Badge = ({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: "neutral" | "green" | "amber" | "red" | "blue";
}) => (
  <span className="badge" data-tone={tone}>
    {children}
  </span>
);

export const Input = (props: InputHTMLAttributes<HTMLInputElement>) => (
  <input className="input" {...props} />
);

export const Field = ({
  label,
  children,
  hint,
}: {
  label: string;
  children: ReactNode;
  hint?: string;
}) => (
  // biome-ignore lint/a11y/noLabelWithoutControl: the control is passed in as `children`
  <label className="field">
    <span className="field-label">{label}</span>
    {children}
    {hint ? <span className="field-hint">{hint}</span> : null}
  </label>
);

export const EmptyState = ({
  icon,
  title,
  hint,
  action,
}: {
  icon?: ReactNode;
  title: string;
  hint?: string;
  action?: ReactNode;
}) => (
  <div className="empty">
    {icon ? <div className="empty-icon">{icon}</div> : null}
    <div className="empty-title">{title}</div>
    {hint ? <div className="empty-hint">{hint}</div> : null}
    {action ? <div className="empty-action">{action}</div> : null}
  </div>
);

export const Spinner = ({ label }: { label?: string }) => (
  <div className="spinner-wrap">
    <span className="spinner" />
    {label ? <span className="spinner-label">{label}</span> : null}
  </div>
);

// A modal dialog — backdrop click and Esc close it.
export const Modal = ({
  title,
  onClose,
  children,
  width = 520,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  width?: number;
}) => {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: backdrop click-to-close; Esc is handled above
    <div className="modal-backdrop" onMouseDown={onClose}>
      {/* biome-ignore lint/a11y/noStaticElementInteractions: stops backdrop close when clicking inside */}
      <div className="modal" style={{ width }} onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span className="modal-title">{title}</span>
          <button type="button" className="modal-close" onClick={onClose} aria-label="Close">
            ✕
          </button>
        </div>
        <div className="modal-body">{children}</div>
      </div>
    </div>
  );
};

// Header for a view: title, count, and trailing actions.
export const ViewHeader = ({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children?: ReactNode;
}) => (
  <div className="view-header">
    <h1 className="view-title">
      {title}
      {count !== undefined ? <span className="view-count">{count}</span> : null}
    </h1>
    <div className="view-header-actions">{children}</div>
  </div>
);
