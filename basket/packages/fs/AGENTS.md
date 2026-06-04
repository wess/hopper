# @basket/fs

Sandboxed file ops scoped to a root directory (typically `paths.data`)
plus a tiny "recently opened" list helper.

## Exports

- `createScope(root)` → `Scope` — file ops that refuse to escape `root`
- `createRecents({ store, key?, limit? })` → `Recents` — recents list backed by `@basket/store`

## Types

```ts
type Scope = {
  root: string
  resolve: (path: string) => string             // absolute, validated
  read: (path) => Promise<string>
  readBytes: (path) => Promise<Uint8Array>
  readJson: <T>(path) => Promise<T>
  write: (path, content) => Promise<void>
  writeJson: (path, value, pretty?) => Promise<void>
  exists: (path) => Promise<boolean>
  remove: (path) => Promise<void>               // recursive
  list: (path?) => Promise<readonly ScopeEntry[]>
  ensureDir: (path?) => Promise<void>
}

type ScopeEntry = {
  name: string
  path: string                                   // relative to root
  absolute: string
  isFile: boolean
  isDirectory: boolean
  size: number
}

type Recents = {
  list: () => readonly string[]
  add: (path: string) => void
  remove: (path: string) => void
  clear: () => void
}
```

## Usage

### Scoped file ops

```ts
import { ensurePaths } from "@basket/config"
import { createScope } from "@basket/fs"

const p = await ensurePaths({ name: "Notes" })
const data = createScope(p.data)

await data.writeJson("settings.json", { theme: "dark" })
const settings = await data.readJson<{ theme: string }>("settings.json")

const entries = await data.list()       // entries directly in data/
const notes = await data.list("notes")  // entries in data/notes/

// Escape attempts throw
data.resolve("../../../etc/passwd")     // ❌ throws "Path escapes scope"
```

The scope rejects paths that resolve outside `root` — guard against
attacker-controlled paths flowing into file ops.

### Recently opened

```ts
import { createStore } from "@basket/store"
import { createRecents } from "@basket/fs"
import { openFile } from "@basket/dialog"

const settings = createStore("settings", { app: { name: "Notes" } })
const recents = createRecents({ store: settings, key: "recentFiles", limit: 20 })

const path = await openFile()
if (path) {
  recents.add(path)
}

// build menu / list view
for (const p of recents.list()) { ... }
```

## Depends on

- `@basket/store` — for `createRecents`
- Bun's `Bun.file` / `Bun.write`, `node:fs/promises` for `mkdir` / `readdir` / `rm` / `stat`
