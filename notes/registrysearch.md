# Multi-registry image search

Hopper's image search fans out across three public registries and merges the
results into a single ranked list, each row carrying a ready-to-pull reference.

## Architecture

The webview invokes one IPC channel, `registry:search`
(`ch.registrySearch`), with `{ term, sources? }`. The host handler
(`src/host/registry/index.ts`) runs the selected providers concurrently and
returns a merged `RegistryResult[]`.

```
hubsearch.tsx ──invoke(registrySearch)──▶ index.ts (handler)
                                            ├─ hub.ts      (Docker Hub)
                                            ├─ github.ts   (GitHub → GHCR)
                                            └─ quay.ts     (Quay.io)
```

Each provider is a plain async function `(term, signal) => RegistryResult[]`
that calls a public HTTP API with `fetch` (not the Docker socket). The handler
wraps each call with a per-provider `AbortController` timeout (~6s) and runs
them under `Promise.allSettled`, so a slow, failing, or rate-limited provider
contributes `[]` and never fails the overall search.

## Providers

| Source       | API                                                       | Stars        | Notes |
| ------------ | --------------------------------------------------------- | ------------ | ----- |
| `dockerhub`  | `hub.docker.com/v2/search/repositories/`                  | `star_count` | Official images flagged. |
| `ghcr`       | `api.github.com/search/repositories` (sorted by stars)    | `stargazers_count` | Requires `User-Agent`; rate-limited. |
| `quay`       | `quay.io/api/v1/find/repositories`                        | `-1` (n/a)   | No popularity signal. |

## How refs and URLs are built

`ref` is what `docker pull` receives; `url` is the human web page.

- **Docker Hub** — `ref` is the bare `repo_name` (e.g. `nginx` for official,
  `bitnami/redis` for namespaced). `url` is `/_/<name>` for official images,
  `/r/<name>` otherwise.
- **GHCR** — `ref` is `ghcr.io/<full_name>`, a **best-effort** pull hint derived
  from the GitHub repo. Not every repository publishes a GHCR image, so the pull
  may 404; `url` points at the GitHub repo page (`html_url`). `updated` carries
  the repo's `pushed_at`.
- **Quay** — `ref` is `quay.io/<namespace>/<name>`, `url` is
  `quay.io/repository/<namespace>/<name>`.

## Merge / rank

The handler:
1. Concatenates all provider results.
2. Dedupes by `ref` (first occurrence wins; provider order is stable).
3. Sorts by `stars` descending — `-1` (unknown, e.g. Quay) sinks to the bottom
   but is kept.
4. Caps at ~40 results.

## Adding a registry

1. Add the literal to `RegistrySource` in `src/shared/types.ts` (contract file).
2. Create `src/host/registry/<name>.ts` exporting
   `search<Name>(term, signal): Promise<RegistryResult[]>` — call the API with
   `fetch`, guard the payload defensively, and return `[]` on any non-OK
   response.
3. Register it in the `PROVIDERS` map and `ALL_SOURCES` in `index.ts`.
4. Add a chip + badge tone in `hubsearch.tsx` (`SOURCES`).

## Rate-limit notes

- **GitHub** unauthenticated search is limited to roughly 10 requests/min. The
  provider sends a `User-Agent` (required) and treats any `403`/`429` (or other
  non-OK) as an empty result rather than an error, so the search degrades to the
  other registries instead of breaking.
- **Docker Hub** and **Quay** search endpoints are unauthenticated and generous,
  but the same defensive non-OK → `[]` handling applies.
- The ~6s per-provider timeout bounds worst-case latency regardless of any
  single registry stalling.
