// Docker Hub provider. Hits the public Hub search API (not the docker socket)
// and projects each repository into a unified `RegistryResult`.

import type { RegistryResult } from "../../shared/types.ts";

type HubRepo = {
  repo_name?: string;
  short_description?: string;
  star_count?: number;
  is_official?: boolean;
};

type HubResponse = {
  results?: HubRepo[];
};

// Build the human web page for a repo. Official images live under `/_/`,
// everyone else under `/r/`.
const hubUrl = (name: string, official: boolean): string =>
  official ? `https://hub.docker.com/_/${name}` : `https://hub.docker.com/r/${name}`;

export const searchHub = async (term: string, signal: AbortSignal): Promise<RegistryResult[]> => {
  const url = `https://hub.docker.com/v2/search/repositories/?query=${encodeURIComponent(
    term,
  )}&page_size=25`;
  const res = await fetch(url, { signal });
  if (!res.ok) return [];
  const body = (await res.json()) as HubResponse;
  const repos = Array.isArray(body.results) ? body.results : [];
  return repos.flatMap((r): RegistryResult[] => {
    const name = r.repo_name;
    if (!name) return [];
    const official = r.is_official === true;
    return [
      {
        source: "dockerhub",
        name,
        ref: name, // official images pull by bare name, others by "owner/repo"
        description: r.short_description ?? "",
        stars: typeof r.star_count === "number" ? r.star_count : 0,
        official,
        url: hubUrl(name, official),
      },
    ];
  });
};
