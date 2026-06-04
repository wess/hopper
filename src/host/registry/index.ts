// Multi-registry image search. Fans out to Docker Hub, GHCR/GitHub, and Quay
// concurrently, then merges, dedupes by pull `ref`, sorts by popularity, and
// caps the result set. A slow or failing provider contributes [] rather than
// failing the whole search.

import { handle } from "@basket/ipc";
import * as ch from "../../shared/channels.ts";
import type { RegistryResult, RegistrySource } from "../../shared/types.ts";
import { searchGithub } from "./github.ts";
import { searchHub } from "./hub.ts";
import { searchQuay } from "./quay.ts";

const PROVIDER_TIMEOUT_MS = 6000;
const MAX_RESULTS = 40;

type Provider = (term: string, signal: AbortSignal) => Promise<RegistryResult[]>;

const PROVIDERS: Record<RegistrySource, Provider> = {
  dockerhub: searchHub,
  ghcr: searchGithub,
  quay: searchQuay,
};

const ALL_SOURCES: RegistrySource[] = ["dockerhub", "ghcr", "quay"];

// Run a single provider with its own abort-based timeout, swallowing any error
// (network, timeout, bad payload) into an empty list.
const runProvider = async (provider: Provider, term: string): Promise<RegistryResult[]> => {
  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), PROVIDER_TIMEOUT_MS);
  try {
    return await provider(term, ac.signal);
  } catch {
    return [];
  } finally {
    clearTimeout(timer);
  }
};

// Dedupe by pull `ref` (first occurrence wins — providers are merged in a
// stable order so this is deterministic).
const dedupe = (results: RegistryResult[]): RegistryResult[] => {
  const seen = new Set<string>();
  const out: RegistryResult[] = [];
  for (const r of results) {
    if (seen.has(r.ref)) continue;
    seen.add(r.ref);
    out.push(r);
  }
  return out;
};

// Sort by stars desc. -1 (unknown popularity, e.g. Quay) sinks below anything
// with a real count but stays in the list.
const byStars = (a: RegistryResult, b: RegistryResult): number => b.stars - a.stars;

export const registerRegistryHandlers = (): void => {
  handle(ch.registrySearch, async ({ term, sources }) => {
    const q = term.trim();
    if (!q) return [];

    const selected = sources && sources.length > 0 ? sources : ALL_SOURCES;
    const settled = await Promise.allSettled(selected.map((src) => runProvider(PROVIDERS[src], q)));

    const merged = settled.flatMap((s) => (s.status === "fulfilled" ? s.value : []));
    return dedupe(merged).sort(byStars).slice(0, MAX_RESULTS);
  });
};
