# @basket/migrate

Schema-diff and versioned migrations for `@basket/db` SQLite databases.

## Exports

- `diff(db, tables)` → `SchemaDiff` — compare declared `defineTable()` schemas against the live DB. Reports new tables, new columns, removed/renamed columns, type changes.
- `sync(db, tables)` → `SyncReport` — apply additive diffs (new tables, new columns) inside a transaction. Returns `{ applied, warnings }`.
- `run(db, migrations)` → `string[]` — apply pending versioned migrations in order. Idempotent; tracks state in `__basket_migrations__`.
- `rollback(db, migrations, to?)` → `string[]` — revert applied migrations down to (and excluding) `to`.
- `status(db, migrations)` → `MigrationStatus[]` — applied/pending list.

## Types

```ts
type SchemaDiff = {
  newTables:      { table; sql }[]
  newColumns:     { table; column; sql }[]
  removedColumns: { table; column; warn }[]
  removedTables:  { table; warn }[]
  typeChanges:    { table; column; warn }[]
}

type Migration = {
  id: string
  up:   string | ((db: DB) => void)
  down?: string | ((db: DB) => void)
}
```

## Usage

### Schema-diff for declarative tables

```ts
import { connect, defineTable, column } from "@basket/db"
import { diff, sync } from "@basket/migrate"

const notes = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
  body: column.text().default(""),
  pinned: column.boolean().default(false),
  // newly added field — sync() picks this up as ALTER TABLE ADD COLUMN
  archivedAt: column.timestamp().nullable(),
})

const db = connect("./app.db")

const d = diff(db, [notes])
// { newTables: [], newColumns: [{ table: "notes", column: "archivedAt", sql: "..." }], … }

const { applied, warnings } = sync(db, [notes])
// applied: ["+ notes.archivedAt"]
// warnings: []
```

`sync()` is **additive-only**: it will create new tables and add new
columns, but never drop or alter existing data. Removed columns, type
changes, and removed tables come back as `warnings`. For destructive
changes, write an explicit `Migration` with the recreate-and-copy
pattern.

### Versioned migrations

```ts
import { run, rollback, status, type Migration } from "@basket/migrate"

const migrations: Migration[] = [
  {
    id: "2026-05-01_pinned",
    up: "ALTER TABLE notes ADD COLUMN pinned INTEGER DEFAULT 0",
    down: "ALTER TABLE notes DROP COLUMN pinned",
  },
  {
    id: "2026-05-08_search_index",
    up: (db) => {
      db.raw.exec("CREATE INDEX idx_notes_title ON notes(title)")
    },
    down: (db) => {
      db.raw.exec("DROP INDEX idx_notes_title")
    },
  },
]

const applied = run(db, migrations)
// → ["2026-05-01_pinned", "2026-05-08_search_index"]

status(db, migrations)
// → [{ id: "2026-05-01_pinned", appliedAt: "2026-05-08 12:00:00" }, ...]

rollback(db, migrations, "2026-05-08_search_index")
// reverts everything *down to* (excluding) that id
```

State is tracked in `__basket_migrations__(id PRIMARY KEY, applied_at)`.
`run()` is fully transactional — if any migration throws, none are
recorded.

## Recommended pattern

Combine both:

1. Use `sync()` on app start for additive schema evolution (new
   columns, new tables) — keeps your `defineTable()` declarations as
   the source of truth.
2. Use `run()` for destructive or data-shaping migrations (renames,
   FK changes, backfills) where you need explicit control.

## Depends on

- `@basket/db` — types and `bun:sqlite` access

Zero external dependencies.
