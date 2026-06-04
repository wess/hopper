# @basket/store

Local JSON key-value store for desktop app settings and state. Atomic writes, persisted under the platform-correct user config directory.

## Exports

- `createStore(name, opts)` → `Store` — disk-backed store at `<config-dir>/<name>.json`
- `memoryStore(defaults?)` → `Store` — in-memory implementation (tests, scratch state)

## Types

```ts
type Store = {
  get: <T>(key: string) => T | undefined
  set: (key: string, value: unknown) => void
  delete: (key: string) => void
  has: (key: string) => boolean
  all: () => Readonly<Record<string, unknown>>
  clear: () => void
  path: string  // absolute path to the json file (or ":memory:")
}

type StoreOptions = {
  app: { name: string; id?: string }    // routed through @basket/config paths()
  defaults?: Record<string, unknown>
}
```

## Usage

```ts
import { createStore } from "@basket/store"

const settings = createStore("settings", {
  app: { name: "Notes", id: "io.wess.notes" },
  defaults: { theme: "system", fontSize: 14 },
})

settings.set("theme", "dark")
const theme = settings.get<string>("theme")  // "dark"
settings.has("missing")                       // false
settings.all()                                // { theme: "dark", fontSize: 14 }
```

The store is loaded from disk asynchronously after construction. Writes
are atomic (write-to-tmp, rename) and queued, so concurrent `set()` calls
are durable in order.

For ephemeral state — open windows, transient UI — prefer `memoryStore()`.

## Recipes

### Window state persistence (used by `@basket/window`)

```ts
const wins = createStore("windows", { app })
wins.set("main", { width: 1200, height: 800, x: 100, y: 80 })
const restored = wins.get<{ width: number; height: number; x?: number; y?: number }>("main")
```

### Encrypted secrets

This store does **not** encrypt values. For secrets (auth tokens, API
keys), use butter's `securestorage` plugin (OS keychain) instead.

## Depends on

- `@basket/config` — for `paths(app).config` to find the user config dir
