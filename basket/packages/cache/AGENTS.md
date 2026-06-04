# @basket/cache

TTL-based caching. Two implementations:

- `memoryCache(options?)` — in-process Map, lost on restart
- `diskCache({ app, file?, ... })` — JSON-on-disk at `paths(app).cache`

Both expose the same `Cache` interface, including a cache-aside helper.

## Exports

- `memoryCache(options?)` → `Cache`
- `diskCache({ app, file?, defaultTtlMs? })` → `Cache`

## Types

```ts
type Cache = {
  get:     <T>(key) => Promise<T | undefined>
  set:     <T>(key, value, ttlMs?) => Promise<void>
  has:     (key) => Promise<boolean>
  delete:  (key) => Promise<void>
  clear:   () => Promise<void>
  aside:   <T>(key, loader, ttlMs?) => Promise<T>   // get-or-load
}
```

## Usage

### Memory cache with default TTL

```ts
import { memoryCache } from "@basket/cache"
const cache = memoryCache({ defaultTtlMs: 60_000 })

await cache.set("recent", { items }) // expires in 60s
const r = await cache.get<{ items: Item[] }>("recent")
```

### Disk cache for cross-launch persistence

```ts
import { diskCache } from "@basket/cache"

const cache = diskCache({
  app: { name: "Notes" },
  defaultTtlMs: 12 * 60 * 60_000,  // 12h
})

// stored at ~/Library/Caches/Notes/cache.json (macOS)
await cache.set("issues", issues)
```

### Cache-aside

```ts
const data = await cache.aside(
  `gh:${repo}`,
  () => api.get(`/repos/${repo}/issues`).then((r) => r.data),
  60_000,
)
```

`aside()` runs the loader only if the entry is missing or expired,
stores the result, and returns it. Use it for "lazy + TTL'd" data
without writing the get/set dance every time.

## Notes

- Disk writes are atomic (write-to-tmp + rename) and queued.
- No eviction policy. Entries persist until they expire or are
  explicitly deleted. For bounded caches, layer LRU on top.
- For Redis-backed shared caches across desktop *and* server, use
  `@atlas/cache` on the server and access it via `@basket/request`.

## Depends on

- `@basket/config` — `diskCache` uses `paths(app).cache`
