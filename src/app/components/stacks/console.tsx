// A small auto-scrolling console for streamed compose output. Renders nothing
// until there's at least one line.

import { useEffect, useRef } from "react";

export const Console = ({ lines }: { lines: readonly string[] }) => {
  const ref = useRef<HTMLPreElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll to bottom on every new line
  useEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [lines.length]);

  if (lines.length === 0) return null;
  return (
    <pre ref={ref} className="build-log">
      {lines.join("\n")}
    </pre>
  );
};
