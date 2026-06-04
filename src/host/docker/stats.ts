// Per-container resource stats. Streams the daemon's stats firehose and
// reduces each raw frame to the display-ready `ContainerStats`.

import type { ContainerStats } from "../../shared/types.ts";
import { ndjson } from "./client.ts";

type RawStats = {
  cpu_stats?: {
    cpu_usage?: { total_usage?: number; percpu_usage?: number[] };
    system_cpu_usage?: number;
    online_cpus?: number;
  };
  precpu_stats?: {
    cpu_usage?: { total_usage?: number };
    system_cpu_usage?: number;
  };
  memory_stats?: {
    usage?: number;
    limit?: number;
    stats?: { cache?: number; inactive_file?: number };
  };
  networks?: Record<string, { rx_bytes?: number; tx_bytes?: number }>;
  blkio_stats?: { io_service_bytes_recursive?: { op?: string; value?: number }[] | null };
  pids_stats?: { current?: number };
};

const reduce = (id: string, s: RawStats): ContainerStats => {
  const cpuTotal = s.cpu_stats?.cpu_usage?.total_usage ?? 0;
  const preCpuTotal = s.precpu_stats?.cpu_usage?.total_usage ?? 0;
  const sys = s.cpu_stats?.system_cpu_usage ?? 0;
  const preSys = s.precpu_stats?.system_cpu_usage ?? 0;
  const onlineCpus = s.cpu_stats?.online_cpus ?? s.cpu_stats?.cpu_usage?.percpu_usage?.length ?? 1;
  const cpuDelta = cpuTotal - preCpuTotal;
  const sysDelta = sys - preSys;
  const cpuPercent = sysDelta > 0 && cpuDelta > 0 ? (cpuDelta / sysDelta) * onlineCpus * 100 : 0;

  const memRaw = s.memory_stats?.usage ?? 0;
  // Match `docker stats`: subtract the page cache from usage.
  const cache = s.memory_stats?.stats?.inactive_file ?? s.memory_stats?.stats?.cache ?? 0;
  const memUsage = Math.max(0, memRaw - cache);
  const memLimit = s.memory_stats?.limit ?? 0;
  const memPercent = memLimit > 0 ? (memUsage / memLimit) * 100 : 0;

  let netRx = 0;
  let netTx = 0;
  for (const n of Object.values(s.networks ?? {})) {
    netRx += n.rx_bytes ?? 0;
    netTx += n.tx_bytes ?? 0;
  }

  let blockRead = 0;
  let blockWrite = 0;
  for (const e of s.blkio_stats?.io_service_bytes_recursive ?? []) {
    if (e.op?.toLowerCase() === "read") blockRead += e.value ?? 0;
    else if (e.op?.toLowerCase() === "write") blockWrite += e.value ?? 0;
  }

  return {
    id,
    cpuPercent: Math.round(cpuPercent * 100) / 100,
    memUsage,
    memLimit,
    memPercent: Math.round(memPercent * 100) / 100,
    netRx,
    netTx,
    blockRead,
    blockWrite,
    pids: s.pids_stats?.current ?? 0,
  };
};

// Stream stats for one container until `signal` aborts. `onSample` fires per
// frame (~once per second from the daemon).
export const stream = async (
  id: string,
  signal: AbortSignal,
  onSample: (s: ContainerStats) => void,
): Promise<void> => {
  for await (const raw of ndjson<RawStats>(`/containers/${id}/stats`, {
    query: { stream: true },
    signal,
  })) {
    onSample(reduce(id, raw));
  }
};
