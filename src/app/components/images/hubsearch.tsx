// Multi-registry image search dialog. Queries Docker Hub, GHCR, and Quay
// together (via the `registrySearch` handler), merges them into one ranked
// list, and lets the user kick off a pull for any result. Pulling delegates
// back to the parent (which opens the Pull dialog pre-filled with the chosen
// reference).

import { Download, Search, Star } from "lucide-react";
import { useState } from "react";
import * as ch from "../../../shared/channels.ts";
import type { RegistryResult, RegistrySource } from "../../../shared/types.ts";
import { errorMessage, invoke } from "../../lib/ipc.ts";
import { Badge, Button, EmptyState, Input, Modal, Spinner } from "../ui.tsx";

type Props = {
  readonly onClose: () => void;
  readonly onPull: (ref: string) => void;
};

type SourceMeta = {
  readonly source: RegistrySource;
  readonly label: string;
  readonly tone: "blue" | "neutral" | "amber";
};

const SOURCES: readonly SourceMeta[] = [
  { source: "dockerhub", label: "Docker Hub", tone: "blue" },
  { source: "ghcr", label: "GHCR", tone: "neutral" },
  { source: "quay", label: "Quay", tone: "amber" },
];

const toneOf = (source: RegistrySource): SourceMeta["tone"] =>
  SOURCES.find((s) => s.source === source)?.tone ?? "neutral";

const labelOf = (source: RegistrySource): string =>
  SOURCES.find((s) => s.source === source)?.label ?? source;

export const HubSearchDialog = ({ onClose, onPull }: Props) => {
  const [term, setTerm] = useState("");
  const [enabled, setEnabled] = useState<readonly RegistrySource[]>(SOURCES.map((s) => s.source));
  const [results, setResults] = useState<readonly RegistryResult[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = (source: RegistrySource): void =>
    setEnabled((cur) =>
      cur.includes(source) ? cur.filter((s) => s !== source) : [...cur, source],
    );

  const run = async (): Promise<void> => {
    const q = term.trim();
    if (!q || enabled.length === 0) return;
    setLoading(true);
    setError(null);
    try {
      setResults(await invoke(ch.registrySearch, { term: q, sources: [...enabled] }));
    } catch (e) {
      setError(errorMessage(e));
      setResults(null);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Modal title="Search Image Registries" onClose={onClose} width={640}>
      <div className="search-input" style={{ maxWidth: "none", marginBottom: 10 }}>
        <Search size={15} />
        <Input
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
          placeholder="Search images across registries…"
          autoFocus
        />
        <Button variant="primary" size="sm" onClick={run}>
          Search
        </Button>
      </div>

      <div style={{ display: "flex", gap: 8, marginBottom: 14 }}>
        {SOURCES.map((s) => {
          const on = enabled.includes(s.source);
          return (
            <button
              key={s.source}
              type="button"
              className="btn"
              data-variant={on ? "primary" : "ghost"}
              data-size="sm"
              onClick={() => toggle(s.source)}
            >
              {s.label}
            </button>
          );
        })}
      </div>

      {loading ? <Spinner label="Searching registries…" /> : null}
      {error ? <div style={{ color: "var(--red)", fontSize: 12.5 }}>{error}</div> : null}

      {results && !loading ? (
        results.length === 0 ? (
          <EmptyState
            icon={<Search size={22} />}
            title="No results"
            hint={`Nothing matched “${term}” in the selected registries.`}
          />
        ) : (
          <table className="table">
            <tbody>
              {results.map((r) => (
                <tr key={`${r.source}:${r.ref}`}>
                  <td>
                    <div
                      className="cell-name"
                      style={{ display: "flex", alignItems: "center", gap: 8 }}
                    >
                      <Badge tone={toneOf(r.source)}>{labelOf(r.source)}</Badge>
                      {r.name}
                      {r.official ? <Badge tone="green">official</Badge> : null}
                    </div>
                    <div
                      className="cell-sub"
                      style={{
                        maxWidth: 400,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {r.description || "—"}
                    </div>
                  </td>
                  <td className="right cell-sub" style={{ whiteSpace: "nowrap" }}>
                    {r.stars >= 0 ? (
                      <>
                        <Star size={12} style={{ verticalAlign: "-2px" }} /> {r.stars}
                      </>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td className="right">
                    <Button size="sm" onClick={() => onPull(r.ref)}>
                      <Download size={13} /> Pull
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )
      ) : null}
    </Modal>
  );
};
