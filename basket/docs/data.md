# Data Guide

How `@basket/db`, `@basket/migrate`, `@basket/store`, `@basket/cache`, `@basket/fs`, and `@basket/logger` fit together.

## The data layers

```
                          ┌─────────────────┐
   secrets / tokens  →   │ @basket/secrets │  (OS keychain)
                          └─────────────────┘
                          ┌─────────────────┐
   user prefs        →   │  @basket/store  │  (json on disk)
                          └─────────────────┘
                          ┌─────────────────┐
   structured data   →   │   @basket/db    │  (bun:sqlite)
                          └─────────────────┘
                          ┌─────────────────┐
   files            →    │   @basket/fs    │  (sandboxed scope)
                          └─────────────────┘
                          ┌─────────────────┐
   TTL'd or hot data →   │  @basket/cache  │  (memory + disk)
                          └─────────────────┘
                          ┌─────────────────┐
   logs             →    │ @basket/logger  │  (rotating files)
                          └─────────────────┘
```

Pick the one whose semantics match what you're storing. Don't put auth tokens in `@basket/store` (plaintext JSON) — they belong in `@basket/secrets`. Don't put settings in `@basket/db` — they belong in `@basket/store`.

## SQLite — `@basket/db`

### Schema

```ts
import { column, defineTable, type RowOf } from "@basket/db"

export const notes = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
  body: column.text().default(""),
  pinned: column.boolean().default(false),
  createdAt: column.timestamp().default("now()"),
  updatedAt: column.timestamp().default("now()"),
  archivedAt: column.timestamp().nullable(),
})

export type Note = RowOf<typeof notes>
// { id: number; title: string; body: string; pinned: boolean;
//   createdAt: string; updatedAt: string; archivedAt: string | null }
```

`column.serial()` is `INTEGER PRIMARY KEY AUTOINCREMENT` — don't add `.primaryKey()` on top. `column.timestamp().default("now()")` resolves to `CURRENT_TIMESTAMP`.

### Connect

```ts
import { connect } from "@basket/db"
import { ensurePaths } from "@basket/config"

const p = await ensurePaths({ name: "Notes" })
const db = connect(`${p.data}/notes.db`)
```

`connect()` sets `journal_mode = WAL` and `foreign_keys = ON` automatically.

### Migrations — two flavours

**Additive only (most cases)** — use `@basket/migrate.sync()`:

```ts
import { sync } from "@basket/migrate"

const report = sync(db, [notes])
// report.applied: ["+ notes.archivedAt"]
// report.warnings: []  (removed/typed-changed cols flagged but not touched)
```

`sync` is idempotent and only does safe operations (CREATE TABLE, ALTER ADD COLUMN). It runs in a transaction.

**Destructive or data-shaping** — use versioned migrations:

```ts
import { run, type Migration } from "@basket/migrate"

const migrations: Migration[] = [
  {
    id: "2026-05-01_pinned",
    up: "ALTER TABLE notes ADD COLUMN pinned INTEGER DEFAULT 0",
    down: "ALTER TABLE notes DROP COLUMN pinned",
  },
  {
    id: "2026-05-08_recreate_with_fts",
    up: (db) => {
      db.raw.exec("CREATE VIRTUAL TABLE notes_fts USING fts5(title, body, content='notes')")
      db.raw.exec("INSERT INTO notes_fts(notes_fts) VALUES ('rebuild')")
    },
  },
]

run(db, migrations)        // applies pending, idempotent
```

State is tracked in `__basket_migrations__`. `rollback(db, migrations, to?)` reverts.

### Queries

```ts
import { from } from "@basket/db"

const all      = db.all(from(notes).order("updatedAt", "desc"))
const pinned   = db.all(from(notes).where((q) => q("pinned").equals(true)))
const titleOnly = db.all(from(notes).select("id", "title"))
//      ^? Array<{ id: number; title: string }>

const one = db.one(from(notes).where((q) => q("id").equals(42)))
//    ^? Note | undefined
```

Chained `.where()` clauses are joined with AND:

```ts
db.all(
  from(notes)
    .where((q) => q("pinned").equals(true))
    .where((q) => q("archivedAt").isNull())
    .order("updatedAt", "desc")
    .limit(50),
)
```

Operators on `q("col")`: `equals`, `notEquals`, `lt`, `lte`, `gt`, `gte`, `like`, `in`, `isNull`, `isNotNull`.

### Insert / update / delete

```ts
const created = db.insert(notes, { title: "Hi" })
//    ^? Note  (RETURNING *)

const rows = db.update(notes, { id: created.id }, { pinned: true })
//    ^? number  (rows affected)

const removed = db.remove(notes, { id: created.id })
//    ^? number  (rows affected)
```

### Raw SQL escape hatch

For complex queries (joins across many tables, window functions, FTS):

```ts
const top = db.query<{ id: number; title: string; score: number }>(
  `SELECT n.id, n.title, COUNT(t.id) AS score
     FROM notes n LEFT JOIN tags t ON t.noteId = n.id
    GROUP BY n.id ORDER BY score DESC LIMIT 10`,
)
```

`db.exec(sql, ...params)` for one-shot statements with no return.

### Transactions

Wrap multi-statement writes in `db.raw.transaction()`:

```ts
db.raw.transaction(() => {
  const note = db.insert(notes, { title: "Hi" })
  db.insert(tags, { noteId: note.id, name: "personal" })
  db.insert(tags, { noteId: note.id, name: "draft" })
})()
```

If any step throws, the whole tx rolls back.

## Preferences — `@basket/store`

For settings, window state, recents, last-selected IDs, anything a user could reasonably view in a text editor:

```ts
import { createStore } from "@basket/store"

const settings = createStore("settings", {
  app: { name: "Notes" },
  defaults: { theme: "system", fontSize: 14 },
})

settings.set("theme", "dark")
const theme = settings.get<string>("theme")
settings.all()  // frozen snapshot
```

Stored at `paths(app).config/<name>.json` with atomic writes (write-to-tmp + rename) and a write queue (concurrent `set()` calls land in order). For tests, swap in `memoryStore()`.

## Secrets — `@basket/secrets`

OS keychain access (macOS Keychain, Linux libsecret, Windows Credential Manager). Use for auth tokens, API keys, OAuth credentials.

```ts
import { createVault } from "@basket/secrets"

const vault = createVault("io.wess.notes")
await vault.set("github.token", token)
const t = await vault.get("github.token")

await vault.setJson("oauth.google", { accessToken, refreshToken })
const creds = await vault.getJson<{ accessToken: string }>("oauth.google")
```

`service` / `key` must match `^[a-zA-Z0-9._-]+$` to keep shell escaping safe.

**Windows caveat**: `get` returns `undefined` because `cmdkey` doesn't expose stored passwords. See [troubleshooting.md](troubleshooting.md#windows-keychain).

## Files — `@basket/fs`

Sandboxed file ops scoped to a directory (usually `paths.data`). Refuses paths that resolve outside the root.

```ts
import { createScope } from "@basket/fs"

const data = createScope(p.data)

await data.writeJson("settings.json", { theme: "dark" })
const settings = await data.readJson<{ theme: string }>("settings.json")
await data.ensureDir("attachments/2026")
const entries = await data.list("attachments/2026")

data.resolve("../../../etc/passwd")  // ❌ throws — escape attempt
```

For "recently opened" lists from dialog picks:

```ts
import { createRecents } from "@basket/fs"

const recents = createRecents({ store: settings, key: "recentFiles", limit: 20 })
recents.add(path)
recents.list()       // most-recent first
```

## Cache — `@basket/cache`

TTL'd memory or disk cache with cache-aside helper:

```ts
import { diskCache } from "@basket/cache"

const cache = diskCache({
  app: { name: "Notes" },
  defaultTtlMs: 12 * 60 * 60_000,  // 12h
})

// get-or-load
const repos = await cache.aside(
  `gh:${user}:repos`,
  () => api.get<Repo[]>(`/users/${user}/repos`).then((r) => r.data),
  10 * 60_000,
)
```

Use `memoryCache()` for ephemeral state (test mocks, short-lived dedup).

Stored at `paths(app).cache/cache.json` with atomic writes. No eviction policy — entries persist until expired or explicitly deleted. For bounded caches, layer LRU on top.

## Logging — `@basket/logger`

File-rotating logger that writes to `paths(app).logs`:

```ts
import { createLogger } from "@basket/logger"

const log = createLogger({
  app: { name: "Notes" },
  level: "info",
  console: Bun.env.NODE_ENV !== "production",
})

log.info("starting", { version: "1.0.0" })
log.warn("rate limited", { url, tries: 3 })

const requestLog = log.child({ requestId: "abc" })
requestLog.info("handling")   // includes { requestId: "abc" }
```

Rotates at `maxSize` bytes (default 5 MB), keeps `keep` rotated files (default 5). Writes are queued asynchronously; call `log.flush()` before exit if every line matters.

## Common patterns

### Bootstrapping data dir on first launch

```ts
import { defineConfig, ensurePaths } from "@basket/config"
import { connect } from "@basket/db"
import { sync } from "@basket/migrate"
import { notes } from "./schema"

const config = defineConfig({ app: { name: "Notes", id: "io.wess.notes" } })
const p = await ensurePaths(config.app)
const db = connect(`${p.data}/notes.db`)
sync(db, [notes])
```

### Layered read: cache → db → API

```ts
const data = await cache.aside(
  `note:${id}`,
  async () => {
    const local = db.one(from(notes).where((q) => q("id").equals(id)))
    if (local) return local
    const remote = await api.get<Note>(`/notes/${id}`).then((r) => r.data)
    db.insert(notes, remote)
    return remote
  },
  60_000,
)
```

### Full-text search

```ts
run(db, [{
  id: "2026-05-01_fts",
  up: (db) => {
    db.raw.exec("CREATE VIRTUAL TABLE notes_fts USING fts5(title, body, content='notes')")
    db.raw.exec("INSERT INTO notes_fts(notes_fts) VALUES ('rebuild')")
  },
}])

const results = db.query<Note>(
  `SELECT n.* FROM notes n
     JOIN notes_fts f ON f.rowid = n.id
    WHERE notes_fts MATCH ?
    ORDER BY rank LIMIT 50`,
  term,
)
```

## Anti-patterns

- **Don't store tokens in `@basket/store`.** It's plaintext JSON. Use `@basket/secrets`.
- **Don't run `sync()` AND a versioned migration that creates the same table.** `sync` will see the table and skip; the versioned migration may fail. Pick one source of truth per table.
- **Don't `db.query()` from a webview** — you can't; only host code has database access. Wrap the query in an IPC handler.
- **Don't forget to `ensurePaths()`** before opening a db on a fresh install. `Bun.file().write()` auto-creates parents for files, but `bun:sqlite` does not.

## Next

- [auth.md](auth.md) — adding user accounts on top of the db
- [`packages/db/AGENTS.md`](../packages/db/AGENTS.md) — full db API
- [`packages/migrate/AGENTS.md`](../packages/migrate/AGENTS.md) — migration patterns
