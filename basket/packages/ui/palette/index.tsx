import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";

export type PaletteCommand = {
  readonly id: string;
  readonly label: string;
  readonly hint?: string;
  readonly icon?: ReactNode;
  readonly keywords?: readonly string[];
  readonly action: () => void | Promise<void>;
};

export type PaletteProps = {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly commands: readonly PaletteCommand[];
  readonly placeholder?: string;
};

const score = (term: string, c: PaletteCommand): number => {
  if (!term) return 1;
  const hay = `${c.label} ${c.hint ?? ""} ${(c.keywords ?? []).join(" ")}`.toLowerCase();
  const t = term.toLowerCase();
  if (hay.startsWith(t)) return 3;
  if (hay.includes(t)) return 2;
  // fuzzy: every char appears in order
  let ti = 0;
  for (const ch of hay) {
    if (ch === t[ti]) ti++;
    if (ti === t.length) return 1;
  }
  return 0;
};

export const Palette = ({ open, onClose, commands, placeholder = "Type a command…" }: PaletteProps) => {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const results = useMemo(() => {
    return [...commands]
      .map((c) => ({ c, s: score(query, c) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.c);
  }, [query, commands]);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSelected(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: reset selection on every query change
  useEffect(() => {
    setSelected(0);
  }, [query]);

  if (!open) return null;

  const run = async (cmd: PaletteCommand) => {
    onClose();
    await cmd.action();
  };

  return (
    <div
      className="basket-palette-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="basket-palette" onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="basket-palette-input"
          placeholder={placeholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              onClose();
            } else if (e.key === "ArrowDown") {
              e.preventDefault();
              setSelected((s) => Math.min(results.length - 1, s + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setSelected((s) => Math.max(0, s - 1));
            } else if (e.key === "Enter") {
              e.preventDefault();
              const cmd = results[selected];
              if (cmd) void run(cmd);
            }
          }}
        />
        <div className="basket-palette-list">
          {results.length === 0 ? (
            <div className="basket-palette-empty">No commands</div>
          ) : (
            results.map((c, i) => (
              <div
                key={c.id}
                className="basket-palette-item"
                data-selected={i === selected}
                onMouseEnter={() => setSelected(i)}
                onClick={() => run(c)}
              >
                {c.icon ? <span>{c.icon}</span> : null}
                <span className="basket-palette-item-label">{c.label}</span>
                {c.hint ? <span className="basket-palette-item-hint">{c.hint}</span> : null}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
};

export const usePaletteShortcut = (toggle: () => void, key = "k"): void => {
  useEffect(() => {
    const fn = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === key.toLowerCase()) {
        e.preventDefault();
        toggle();
      }
    };
    window.addEventListener("keydown", fn);
    return () => window.removeEventListener("keydown", fn);
  }, [toggle, key]);
};
