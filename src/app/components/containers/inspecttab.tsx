// Inspect tab — loads the full inspect dump and renders it as a JSON tree.

import * as ch from "../../../shared/channels.ts";
import { useLoad } from "../../lib/ipc.ts";
import { Inspect } from "../inspect.tsx";
import { Spinner } from "../ui.tsx";

export const InspectTab = ({ id }: { id: string }) => {
  const { data, error, loading } = useLoad(ch.containerInspect, { id }, [id]);

  if (loading) return <Spinner label="Loading inspect…" />;
  if (error) return <div className="cell-sub">{error}</div>;
  if (!data) return <div className="cell-sub">No data.</div>;
  return <Inspect data={data} />;
};
