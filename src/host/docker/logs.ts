// Container log streaming. Follows a container's logs, demuxing the stdcopy
// frames, and invokes `onLine` for each text chunk until `signal` aborts.

import { streamLogs } from "./client.ts";
import { logsPath } from "./containers.ts";

export const stream = async (
  id: string,
  tail: number,
  signal: AbortSignal,
  onLine: (text: string, stream: "stdout" | "stderr") => void,
): Promise<void> => {
  await streamLogs(
    logsPath(id),
    {
      query: { follow: true, stdout: true, stderr: true, tail, timestamps: false },
      signal,
    },
    onLine,
  );
};
