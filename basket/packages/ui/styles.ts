export const STYLES = `
:root[data-basket-ui] {
  --basket-bg: #fafafa;
  --basket-bg-elevated: #ffffff;
  --basket-bg-overlay: rgba(0, 0, 0, 0.04);
  --basket-fg: #111;
  --basket-fg-muted: #6b6b6b;
  --basket-border: #e3e3e3;
  --basket-selected: #eef4ff;
  --basket-accent: #3b82f6;
  --basket-radius: 10px;
  --basket-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
  --basket-font: -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", sans-serif;
}

:root[data-basket-ui][data-basket-theme="dark"] {
  --basket-bg: #0f0f10;
  --basket-bg-elevated: #1a1a1c;
  --basket-bg-overlay: rgba(255, 255, 255, 0.05);
  --basket-fg: #f0f0f0;
  --basket-fg-muted: #888;
  --basket-border: #2a2a2c;
  --basket-selected: #1f2a3a;
}

.basket-titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 38px;
  padding: 0 0.75rem;
  background: var(--basket-bg-elevated);
  border-bottom: 1px solid var(--basket-border);
  -webkit-app-region: drag;
  user-select: none;
  font: 13px var(--basket-font);
}
.basket-titlebar [data-traffic-spacer] { width: 72px; }
.basket-titlebar [data-no-drag] { -webkit-app-region: no-drag; }
.basket-titlebar-title {
  flex: 1;
  text-align: center;
  font-weight: 600;
  color: var(--basket-fg);
}
.basket-titlebar-actions {
  display: flex;
  gap: 4px;
  -webkit-app-region: no-drag;
}

.basket-sidebar-layout {
  display: grid;
  grid-template-columns: var(--basket-sidebar-width, 260px) 1fr;
  height: 100%;
  font: 14px var(--basket-font);
  color: var(--basket-fg);
  background: var(--basket-bg);
}
.basket-sidebar {
  border-right: 1px solid var(--basket-border);
  background: var(--basket-bg-elevated);
  overflow-y: auto;
}
.basket-detail {
  overflow-y: auto;
}

.basket-palette-backdrop {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: grid;
  place-items: start center;
  padding-top: 12vh;
  z-index: 9999;
  font: 14px var(--basket-font);
}
.basket-palette {
  width: min(560px, 90vw);
  background: var(--basket-bg-elevated);
  color: var(--basket-fg);
  border-radius: var(--basket-radius);
  box-shadow: var(--basket-shadow);
  border: 1px solid var(--basket-border);
  overflow: hidden;
}
.basket-palette-input {
  width: 100%;
  border: none;
  outline: none;
  padding: 0.95rem 1.1rem;
  font-size: 0.95rem;
  background: transparent;
  color: var(--basket-fg);
  border-bottom: 1px solid var(--basket-border);
}
.basket-palette-list { max-height: 50vh; overflow-y: auto; }
.basket-palette-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.55rem 1rem;
  cursor: pointer;
}
.basket-palette-item[data-selected="true"] { background: var(--basket-selected); }
.basket-palette-item-label { flex: 1; }
.basket-palette-item-hint { color: var(--basket-fg-muted); font-size: 0.75rem; }
.basket-palette-empty {
  padding: 1.5rem 1rem;
  text-align: center;
  color: var(--basket-fg-muted);
}

.basket-toaster {
  position: fixed;
  bottom: 1.25rem;
  right: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  z-index: 9998;
  pointer-events: none;
  font: 14px var(--basket-font);
}
.basket-toast {
  pointer-events: auto;
  background: var(--basket-bg-elevated);
  color: var(--basket-fg);
  border: 1px solid var(--basket-border);
  border-radius: var(--basket-radius);
  padding: 0.7rem 0.9rem;
  box-shadow: var(--basket-shadow);
  min-width: 240px;
  max-width: 360px;
  animation: basket-toast-in 180ms ease-out;
}
.basket-toast[data-variant="error"] { border-left: 3px solid #ef4444; }
.basket-toast[data-variant="success"] { border-left: 3px solid #10b981; }
.basket-toast[data-variant="info"] { border-left: 3px solid var(--basket-accent); }
.basket-toast-title { font-weight: 600; margin-bottom: 0.15rem; }
.basket-toast-description { color: var(--basket-fg-muted); font-size: 0.85rem; }

@keyframes basket-toast-in {
  from { opacity: 0; transform: translateX(8px); }
  to   { opacity: 1; transform: translateX(0); }
}
`;
