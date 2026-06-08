// GitHub / GHCR provider. Searches GitHub repositories (sorted by stars) and
// derives a best-effort GHCR pull reference. Note: not every repository
// actually publishes a `ghcr.io` image — the `ref` is a hint, not a guarantee.
// Unauthenticated search is rate-limited (~10/min); 403/429 yields [].

import type { RegistryResult } from "../../shared/types.ts";
import { githubToken } from "../auth/github.ts";

type GitHubRepo = {
  full_name?: string;
  description?: string | null;
  stargazers_count?: number;
  html_url?: string;
  pushed_at?: string;
};

type GitHubResponse = {
  items?: GitHubRepo[];
};

export const searchGithub = async (
  term: string,
  signal: AbortSignal,
): Promise<RegistryResult[]> => {
  const url = `https://api.github.com/search/repositories?q=${encodeURIComponent(
    term,
  )}&sort=stars&order=desc&per_page=20`;
  const headers: Record<string, string> = {
    "User-Agent": "Hopper",
    Accept: "application/vnd.github+json",
  };
  // A connected account lifts the unauthenticated ~10/min search cap and lets
  // private repos surface.
  const token = await githubToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await fetch(url, { signal, headers });
  // Rate-limited or otherwise unavailable — degrade quietly.
  if (!res.ok) return [];
  const body = (await res.json()) as GitHubResponse;
  const items = Array.isArray(body.items) ? body.items : [];
  return items.flatMap((r): RegistryResult[] => {
    const full = r.full_name;
    if (!full) return [];
    return [
      {
        source: "ghcr",
        name: full,
        ref: `ghcr.io/${full}`, // best-effort GHCR pull hint
        description: r.description ?? "",
        stars: typeof r.stargazers_count === "number" ? r.stargazers_count : 0,
        official: false,
        url: r.html_url ?? `https://github.com/${full}`,
        updated: r.pushed_at,
      },
    ];
  });
};
