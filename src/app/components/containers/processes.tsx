// `docker top` process table for a running container.

import * as ch from "../../../shared/channels.ts";
import { useLoad } from "../../lib/ipc.ts";
import { Spinner } from "../ui.tsx";

export const Processes = ({ id }: { id: string }) => {
  const { data, error, loading } = useLoad(ch.containerTop, { id }, [id]);

  if (loading) return <Spinner label="Loading processes…" />;
  if (error) return <div className="cell-sub">{error}</div>;
  if (!data || data.processes.length === 0) return <div className="cell-sub">No processes.</div>;

  return (
    <table className="table">
      <thead>
        <tr>
          {data.titles.map((t) => (
            <th key={t}>{t}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {data.processes.map((row, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: process rows have no stable id.
          <tr key={i}>
            {row.map((cell, j) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: cells are positional.
              <td key={j} className="cell-mono">
                {cell}
              </td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
};
