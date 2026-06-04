# @basket/db

SQLite query builder, schemas, migrations. Backed by `bun:sqlite`.

## Exports

- `connect(path)` → `DB` — opens a SQLite db (sets `journal_mode = WAL` and `foreign_keys = ON`)
- `defineTable(name, columns)` → `Table` — declarative table schema
- `column.{text,integer,real,blob,boolean,timestamp,serial}()` — column factories with chainable modifiers
- `from(table)` → `Buildable<Row>` — query builder
- `migrate(db, tables)` — `CREATE TABLE IF NOT EXISTS` for each table in a transaction
- `tableSql(table)` → `string` — render a single CREATE TABLE statement
- `renderQuery(q)` → `{ sql, params }` — render a `Query` to SQL + params

## Types

```ts
type Col<T> = {
  type: ColumnType
  primaryKey: () => Col<T>
  unique: () => Col<T>
  nullable: () => Col<T | null>
  default: (v: string | number | boolean | null) => Col<T>
  // …plus the underlying flags
}

type Table<C> = { name: string; columns: C }
type RowOf<T> = ...  // recursively maps Col<V> to V

type DB = {
  raw: Database                              // bun:sqlite handle
  all: <R>(q: Query<R>) => R[]
  one: <R>(q: Query<R>) => R | undefined
  insert: <T>(table, data: Partial<RowOf<T>>) => RowOf<T>     // RETURNING *
  update: <T>(table, where: Partial<RowOf<T>>, data: Partial<RowOf<T>>) => number
  remove: <T>(table, where: Partial<RowOf<T>>) => number
  query: <R>(sql, ...params) => R[]          // raw SQL escape hatch
  exec: (sql, ...params) => void
  close: () => void
}
```

## Usage

### Schema

```ts
import { column, defineTable, type RowOf } from "@basket/db"

export const notes = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
  body: column.text().default(""),
  pinned: column.boolean().default(false),
  createdAt: column.timestamp().default("now()"),
  archivedAt: column.timestamp().nullable(),
})

export type Note = RowOf<typeof notes>
// { id: number; title: string; body: string; pinned: boolean; createdAt: string; archivedAt: string | null }
```

### Connect + migrate

```ts
import { connect, migrate } from "@basket/db"
import { paths } from "@basket/config"
import { notes } from "./schema"

const dir = paths({ name: "Notes" }).data
await Bun.write(`${dir}/.keep`, "")  // ensure dir exists
const db = connect(`${dir}/notes.db`)
migrate(db, [notes])
```

### Query

```ts
import { from } from "@basket/db"

const all = db.all(from(notes).order("createdAt", "desc"))
const pinned = db.all(from(notes).where((q) => q("pinned").equals(true)))
const note = db.one(from(notes).where((q) => q("id").equals(1)))

// projection
const titles = db.all(from(notes).select("id", "title"))
//    ^? Array<{ id: number; title: string }>

// chained predicates (AND)
const recent = db.all(
  from(notes)
    .where((q) => q("archivedAt").isNull())
    .where((q) => q("createdAt").gte("2025-01-01"))
    .order("createdAt", "desc")
    .limit(50),
)
```

### Insert / update / delete

```ts
const created = db.insert(notes, { title: "Hi", body: "world" })
//    ^? Note  (RETURNING *)

db.update(notes, { id: created.id }, { pinned: true })

db.remove(notes, { id: created.id })
```

### Raw SQL escape hatch

```ts
const top = db.query<{ count: number }>(
  "SELECT COUNT(*) AS count FROM notes WHERE pinned = ?",
  true,
)
```

## Notes

- `column.serial()` → `INTEGER PRIMARY KEY AUTOINCREMENT`. Don't add
  `.primaryKey()` on top — it's implied.
- `default("now()")` resolves to `CURRENT_TIMESTAMP` for `timestamp`
  columns.
- `connect()` enables WAL mode and foreign-key enforcement by default.
- `migrate()` is idempotent (`IF NOT EXISTS`). It does **not** detect
  schema changes — use it for new tables. Add SQL migrations for
  alterations.

## Depends on

- `bun:sqlite` (built into Bun)

Zero external dependencies.
