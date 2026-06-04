# Concepts

The five ideas you need to internalize to be productive in basket.

## 1. Host vs webview

A butter app is **two bundles**:

- **Host** — runs in Bun. Has the database, filesystem, OS APIs, network. Owns business logic and IPC handlers.
- **Webview** — runs in the OS's native webview (WKWebView, WebView2, WebKitGTK). Has the DOM, your UI, and the `butter` global for talking to the host.

Each side has its own bundle, its own `tsconfig` view, and its own runtime. **They cannot share variables.** They communicate exclusively through IPC.

```
┌─────────────────────────┐         ┌─────────────────────────┐
│  webview (WebView)      │         │  host (Bun)             │
│  src/app/*              │  ◀───▶  │  src/host/*             │
│  DOM, your UI           │   IPC   │  db, fs, network        │
└─────────────────────────┘         └─────────────────────────┘
```

What runs where:

| Package | Host | Webview |
|---|---|---|
| `@basket/config`, `store`, `db`, `migrate`, `fs`, `cache`, `logger` | ✅ | ❌ |
| `@basket/window`, `menu`, `tray`, `dialog`, `notify`, `shortcut`, `protocol`, `lifecycle`, `theme` | ✅ | ❌ |
| `@basket/request`, `secrets`, `auth`, `update`, `ai`, `mcp` | ✅ | ❌ |
| `@basket/ipc` | ✅ | (via `@basket/ipc/client`) |
| `@basket/ui` | ❌ | ✅ |

Mental rule: **anything that touches an OS resource lives on the host.** The webview is the renderer.

## 2. Shared channel definitions

Define IPC channels **once** in a `src/shared/` file. Both the host and the webview import the same `Channel<I, O>` definitions. This is the single most important pattern in basket — it gives you tsserver-checked end-to-end IPC.

```ts
// src/shared/channels.ts
import { defineChannel, defineEvent } from "@basket/ipc"
import type { Note } from "./types"

export const listNotes  = defineChannel<void, Note[]>("notes:list")
export const createNote = defineChannel<{ title: string }, Note>("notes:create")
export const noteCreated = defineEvent<Note>("notes:created")
```

**Channel** = request/response, like an RPC. **Event** = one-way broadcast.

Host implements channels with `handle()`:

```ts
// src/host/index.ts
import { handle, emit } from "@basket/ipc"
import { createNote, noteCreated } from "../shared/channels"

handle(createNote, ({ title }) => {
  const note = db.insert(notesTable, { title })
  emit(noteCreated, note)
  return note
})
```

Webview calls them with `invoke()`:

```ts
// src/app/main.ts
import { invoke, subscribe } from "@basket/ipc/client"
import { createNote, noteCreated } from "../shared/channels"

const note = await invoke(createNote, { title: "Hi" })  // typed result
subscribe(noteCreated, (n) => { /* react to host event */ })
```

Renaming a channel propagates to both sides via tsserver. Changing the payload shape is a type error on both ends until both sides agree.

## 3. Functional, immutable, no classes

Basket follows the same conventions as atlas:

- **No `class`.** Use closures, factory functions, and tagged objects. Errors are the only exception (`Error` must be a class to throw with a stack trace) — but basket tags Error instances via `Object.assign` rather than subclassing.
- **Immutable inputs.** Functions never mutate their arguments. `createStore`, `createTheme`, `createCache` all return new objects on every state transition.
- **Composition over inheritance.** Want middleware around a handler? `pipeline(validate(schema), requireUnlocked)(handler)`. Want to extend a feature? Wrap it in a function.

This is non-negotiable. If you see yourself reaching for `class`, write a factory function that returns a tagged object instead:

```ts
// ❌ no
class Counter {
  private n = 0
  inc() { this.n++ }
  value() { return this.n }
}

// ✅ yes
const createCounter = () => {
  let n = 0
  return {
    inc: () => { n++ },
    value: () => n,
  }
}
```

## 4. Bun first, then butter, then nothing else

When you reach for an API:

1. Is there a `Bun.*` API? → Use it. (`Bun.file`, `Bun.write`, `Bun.spawn`, `Bun.$`, `bun:sqlite`, `Bun.password`, `Bun.serve`.)
2. Is there a Web platform API? → Use it. (`fetch`, `crypto.subtle`, `URL`, `AbortController`.)
3. Is there a `butter` API? → Use it. (`on`, `send`, `setMenu`, `createWindow`, plus `butter/dialog`.)
4. Otherwise, is there a `@basket/*` package that already wraps the above? → Use it.
5. Only then reach for an external dep, and document the **why** in the package's `AGENTS.md`.

Basket's runtime deps total: `@modelcontextprotocol/sdk` (in `@basket/mcp`), `react` + `lucide-react` (peer in `@basket/ui`). Everything else is Bun + Web platform.

## 5. Paths are platform-correct, and you must `ensurePaths`

The OS expects app data in specific places:

| Role | macOS | Linux (XDG) | Windows |
|---|---|---|---|
| `data`  | `~/Library/Application Support/<name>` | `$XDG_DATA_HOME/<name>` | `%APPDATA%\<name>` |
| `config`| `~/Library/Preferences/<name>` | `$XDG_CONFIG_HOME/<name>` | `%APPDATA%\<name>\Config` |
| `cache` | `~/Library/Caches/<name>` | `$XDG_CACHE_HOME/<name>` | `%LOCALAPPDATA%\<name>\Cache` |
| `logs`  | `~/Library/Logs/<name>` | `$XDG_STATE_HOME/<name>/logs` | `%LOCALAPPDATA%\<name>\Logs` |

`paths(app)` returns these. **It does not create them.** Call `await ensurePaths(app)` at startup before any file ops:

```ts
import { defineConfig, ensurePaths } from "@basket/config"

const config = defineConfig({ app: { name: "Notes", id: "io.wess.notes" } })
const p = await ensurePaths(config.app)
const db = connect(`${p.data}/notes.db`)
```

`@basket/store`, `@basket/cache`, and `@basket/logger` all route through `paths()` automatically — they only need the `app` field.

## Putting it together

The minimum viable basket app:

```ts
// src/host/index.ts
import { defineConfig, ensurePaths } from "@basket/config"
import { handle } from "@basket/ipc"
import { mainWindow } from "@basket/window"
import { greet } from "../shared/channels"

const config = defineConfig({ app: { name: "Hello", id: "com.example.hello" } })
await ensurePaths(config.app)

mainWindow({ defaults: { width: 800, height: 600, title: config.app.name } })

handle(greet, ({ name }) => `Hello, ${name}!`)
```

```ts
// src/app/main.ts
import { invoke } from "@basket/ipc/client"
import { greet } from "../shared/channels"

document.body.textContent = await invoke(greet, { name: "world" })
```

Five concepts, ten lines, end-to-end typed.

## Next

- [getting-started.md](getting-started.md) — install and scaffold
- [ipc.md](ipc.md) — deeper on channels, pipelines, validation, errors
- [data.md](data.md) — db, migrations, cache, fs, logger
- [overview.md](overview.md) — full architecture
