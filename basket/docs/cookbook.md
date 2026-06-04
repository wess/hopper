# Cookbook

Patterns and recipes that don't justify a package.

## Persist window state on shutdown

Butter's `lifecycle` plugin emits `before-quit`. Wire it to `win.save()`:

```ts
import { on } from "butter"
const win = mainWindow({ defaults: { width: 1200, height: 800 }, store: settings })

on("lifecycle:before-quit", () => {
  win.save()
})
```

For a fallback, also save on the host process's `beforeExit`:

```ts
process.on("beforeExit", () => win.save())
```

## Periodic snapshot for high-fidelity restore

If users frequently force-quit, snapshot every minute:

```ts
setInterval(() => win.save(), 60_000)
```

The store batches and atomically writes — concurrent saves are safe.

## Toggle dark/light theme via menu

```ts
import { emit } from "@basket/ipc"
import { applyMenu, item, onMenu, section } from "@basket/menu"
import { themeChanged } from "./shared/channels"

applyMenu([
  section("View", [
    item("Toggle Theme", "theme:toggle", { shortcut: "CmdOrCtrl+T" }),
  ]),
])

onMenu("theme:toggle", () => {
  const next = settings.get<"light" | "dark">("theme") === "dark" ? "light" : "dark"
  settings.set("theme", next)
  emit(themeChanged, { theme: next })
})
```

Webview side:

```ts
import { subscribe } from "@basket/ipc/client"
import { themeChanged } from "../shared/channels"

subscribe(themeChanged, ({ theme }) => {
  document.body.dataset.theme = theme
})
```

## Migrations beyond CREATE TABLE

`migrate(db, tables)` is `CREATE TABLE IF NOT EXISTS` only — it doesn't
alter columns. For evolving schemas, use butter's `migrations.run()`
pattern (or write SQL):

```ts
const VERSION_KEY = "schema:version"
const targetVersion = 3

const current = (settings.get<number>(VERSION_KEY) ?? 0)

if (current < 1) db.raw.exec("ALTER TABLE notes ADD COLUMN pinned INTEGER DEFAULT 0")
if (current < 2) db.raw.exec("CREATE INDEX idx_notes_updated ON notes(updatedAt)")
if (current < 3) db.raw.exec("ALTER TABLE notes ADD COLUMN archivedAt TEXT")
settings.set(VERSION_KEY, targetVersion)
```

For complex migrations, prefer a real migrations package (planned:
`@basket/migrate`).

## Open a secondary window

```ts
import { openWindow } from "@basket/window"

const editor = openWindow({
  url: "/editor.html",
  defaults: { width: 900, height: 700, title: "Editor" },
  store: settings,
  storeKey: "editor",
})

// later
editor.close()
```

## Global shortcuts (system-wide)

Butter ships a `globalshortcuts` plugin. Add `globalshortcuts` to
`butter.yaml#plugins` then call butter's plugin API. Basket doesn't wrap
this yet — see butter docs.

## Notifications

Add `notifications` to `butter.yaml#plugins`. From host:

```ts
// uses butter's runtime directly
const r = (globalThis as any).__butterRuntime
r.tell("notifications:show", { title: "New note", body: "Untitled created." })
```

A `@basket/notify` wrapper is on the roadmap.

## Bundle a custom icon for the tray

```ts
import { createTray } from "@basket/tray"

createTray({
  title: "My App",
  icon: new URL("../assets/tray.png", import.meta.url).pathname,
  items: [...],
})
```

The asset gets embedded by `butter compile`.

## Search across rows

```ts
const results = db.all(
  from(notes).where((q) => q("title").like(`%${term}%`))
)
```

For ranked full-text, switch to FTS5:

```ts
db.raw.exec("CREATE VIRTUAL TABLE notes_fts USING fts5(title, body, content='notes')")
db.raw.exec("INSERT INTO notes_fts(notes_fts) VALUES ('rebuild')")
const results = db.query<Note>("SELECT * FROM notes WHERE id IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?)", term)
```

## Encrypted secrets

Don't put auth tokens in `@basket/store` — it's plain JSON on disk. Use
butter's `securestorage` plugin (OS keychain/Credential Manager). A
`@basket/secrets` wrapper is planned.
