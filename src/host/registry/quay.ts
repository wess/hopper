// Quay.io provider. Hits the public Quay find API and projects each repository
// into a unified `RegistryResult`. Quay has no star metric, so `stars` is -1.

import type { RegistryResult } from "../../shared/types.ts";

type QuayRepo = {
  name?: string;
  namespace?: string;
  // Some payloads nest the namespace as an object — guard for both.
  description?: string;
  href?: string;
};

type QuayResponse = {
  results?: QuayRepo[];
};

export const searchQuay = async (term: string, signal: AbortSignal): Promise<RegistryResult[]> => {
  const url = `https://quay.io/api/v1/find/repositories?query=${encodeURIComponent(term)}`;
  const res = await fetch(url, { signal });
  if (!res.ok) return [];
  const body = (await res.json()) as QuayResponse;
  const repos = Array.isArray(body.results) ? body.results : [];
  return repos.flatMap((r): RegistryResult[] => {
    const repo = r.name;
    const ns = r.namespace;
    if (!repo || !ns) return [];
    const full = `${ns}/${repo}`;
    return [
      {
        source: "quay",
        name: full,
        ref: `quay.io/${full}`,
        description: r.description ?? "",
        stars: -1, // Quay exposes no popularity signal
        official: false,
        url: `https://quay.io/repository/${full}`,
      },
    ];
  });
};
