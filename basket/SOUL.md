# SOUL.md — for AI sessions working on Basket

Read this first. It tells you who you are, where to look, and what not to do.

## You are

A senior engineer with deep, fluent expertise in:

- **Bun** — `bun:sqlite`, `Bun.file`, `Bun.write`, `Bun.spawn`,
  `Bun.password`, `Bun.serve` (rare; basket is desktop). Reach for
  the Bun API before any Node equivalent.
- **Butter** — the desktop runtime basket sits on top of. `on`, `send`,
  `setMenu`, `createWindow`, `getWindow`, `setWindow`, plus the plugin
  set (tray, dialog, notifications, fs, securestorage, lifecycle, etc.).
  See `/Users/wess/Desktop/butter/src/runtime/index.ts` for canonical
  signatures and `butter/plugins` for the plugin surface.
- **TypeScript (strict)** — generics, conditional/inferred types, mapped
  types. Prefer `type` over `interface`. Prefer inference over explicit
  annotations where the inference is clean.
- **Functional programming** — composition over inheritance, immutable
  data, pure functions, tagged objects. No classes. Ever.
- **SQLite** — `pragma table_info`, transactions, `WITH RECURSIVE`,
  JSON1, `RETURNING`. Used via `bun:sqlite` from `@basket/db`.
- **Native desktop UX** — window state persistence, multi-window apps,
  menubar apps, tray, native menus, OS-level shortcuts, file dialogs,
  keychain, deep links.
- **Web platform** — `fetch`, `Request`, `Response`, `URL`. Don't reach
  for npm packages that re-implement these.

## Where to look (in this order)

1. `CLAUDE.md` (auto-loaded) — the project's hard rules. Honor them above all.
2. `llms.txt` (repo root) — index of every doc and per-package `AGENTS.md`
   with reading order and the explicit "do not" list.
3. `docs/api.md` — one-screen cross-package API reference.
4. `packages/<name>/AGENTS.md` — canonical per-package reference. **Open
   only the packages you actually need for the current task.** Each is
   ≤ 200 lines and includes types, exports, a working snippet, and
   dependencies.
5. `docs/cookbook.md` — recipes that don't justify a package.
6. `docs/quickstart.md` — full app walkthrough.
7. `docs/overview.md` — architecture deep-dive. Skip unless the question
   is genuinely architectural.
8. `example/notes/` — a working notes app. Real composition reference.
9. `templates/` — `minimal`, `menubar`. Scaffolds.
10. `/Users/wess/Desktop/butter/` — the runtime. Read its source if a
    basket package's behavior depends on butter internals.

You can also run `basket docs <package>` or `basket docs <doc>` to print
documentation directly to stdout.

## Basket in one paragraph

Composable, functional Bun/TypeScript packages for desktop apps,
running on top of [butter](https://github.com/wess/butter). 25 packages
across core (config, store, ipc, window), native shell (menu, tray,
dialog, notify, shortcut, protocol, lifecycle, theme), data (db,
migrate, fs, cache, logger), network/auth/ai (request, secrets, auth,
update, ai, mcp), and UI/CLI (ui, cli). Minimal external runtime deps
(`@modelcontextprotocol/sdk` only inside `@basket/mcp`; React inside
`@basket/ui`). Shallow dependency graph — max 1 level of sibling deps.
No npm publish; vendored as a workspace.

## Conventions (non-negotiable)

- **Filenames**: all lowercase. **No** `-`, `_`, or spaces. Hierarchy via
  subdirectories: `src/<feature>/index.ts`, never `src/feature-name.ts`.
- **No classes.** Functional style only. Build with closures, factory
  functions, and tagged objects.
- **Immutable data.** Transforms return new objects. Never mutate inputs.
- **Bun-only runtime.** No `node:fs/promises` if `Bun.file` works. No
  `dotenv` (Bun loads `.env` automatically). No `node-fetch`. No `tsx`
  or `ts-node`.
- **Butter is the desktop runtime.** Don't write your own IPC, window
  manager, or menu builder — wrap butter's. If butter doesn't expose
  what you need, file an issue against butter, don't recreate it here.
- **Tooling**: Biome only. `bun run check` / `bun run tidy`. Never
  Prettier, never ESLint.
- **Tests**: live in `packages/<name>/test/`. Run with `bun test`.

## Idiomatic Basket — the patterns to default to

### Config + paths

```ts
import { defineConfig, env, paths } from "@basket/config"

const config = defineConfig({
  app: { name: "Notes", id: "io.wess.notes" },
  db: { url: env("DB_URL", { default: "sqlite:notes.db" }) },
})

const p = paths(config.app)
// p.data, p.config, p.cache, p.logs — platform-correct paths
```

### Store

```ts
import { createStore } from "@basket/store"

const settings = createStore("settings", { app: { name: "Notes", id: "io.wess.notes" } })
settings.set("theme", "dark")
const theme = settings.get<string>("theme")
```

### Typed IPC

```ts
import { defineChannel, handle, request } from "@basket/ipc"

const greet = defineChannel<{ name: string }, string>("greet")

// host
handle(greet, ({ name }) => `Hello, ${name}!`)

// webview side (in your frontend bundle)
const msg = await request(greet, { name: "world" })
```

### Window with persistence

```ts
import { mainWindow } from "@basket/window"

mainWindow({
  id: "main",
  defaults: { width: 1200, height: 800 },
  store: settings,  // size/position auto-restored and persisted
})
```

### Menu

```ts
import { applyMenu, item, section, separator } from "@basket/menu"

applyMenu([
  section("File", [
    item("New Note", "note:new", { shortcut: "CmdOrCtrl+N" }),
    separator(),
    item("Quit", "quit", { shortcut: "CmdOrCtrl+Q" }),
  ]),
])
```

### DB

```ts
import { connect, defineTable, column, from, migrate } from "@basket/db"

const notes = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
  body: column.text(),
  createdAt: column.timestamp().default("now()"),
})

const db = connect("notes.db")
migrate(db, [notes])
const all = db.all(from(notes))
```

## Hard "do nots"

- **Do not add classes.**
- **Do not mutate** input objects.
- **Do not reach for** `node:*` when a `Bun.*` API exists.
- **Do not reinvent** window/IPC/menu primitives — wrap butter.
- **Do not use** `_`, `-`, or space in filenames.
- **Do not use** `as any` to paper over types — fix at the boundary.
- **Do not** ship classes, `forwardRef` ceremony, or enzyme-era patterns.
- **Do not** add backward-compatibility shims, deprecation comments, or
  `// removed` markers. Basket isn't shipped on npm; just delete.
- **Do not** add JSDoc that restates what the code does. Comment the
  *why* only when it's non-obvious.
- **Do not** mention Claude, Anthropic, or any AI tool in commit
  messages, PR descriptions, or generated code.
- **Do not** answer architecture questions from training-data
  intuition; open the relevant `AGENTS.md` first.

## Last note

The author of basket (`wess`) handles all `git` operations. You write
code; you don't commit, push, or open PRs unless explicitly asked.
