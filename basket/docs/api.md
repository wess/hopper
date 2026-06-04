# Basket API — one-screen reference

Cross-package quick lookup. For depth, open `packages/<name>/AGENTS.md`.

## Core

### @basket/config

```ts
defineConfig(schema)                 // → frozen, resolved config
env(name, { parse?, default? })      // → EnvRef<T>
paths({ name, id? })                 // → { data, config, cache, logs }
await ensurePaths({ name, id? })     // creates the four dirs, returns paths
```

### @basket/store

```ts
createStore(name, { app, defaults? })  // → Store (json on disk)
memoryStore(defaults?)                  // → Store (in-memory)

store.get<T>(key) | set(k, v) | delete(k) | has(k) | all() | clear()
store.path                              // path to json file or ":memory:"
```

### @basket/ipc

```ts
// shared
defineChannel<I, O>(name)              // → Channel<I, O>
defineEvent<T>(name)                   // → Event<T>

// host
import { handle, emit, listen, pipeline, validate, notFound, … } from "@basket/ipc"
handle(channel, (input) => output, { input?: validator })
emit(event, data)
listen(event, (data) => void)

// pipes
pipeline(...pipes)(handler)            // returns a Handler<I, O>
validate(schema)                        // pipe that schema.parse(ctx.input)
assign(ctx, "key", value)               // pure assigns helper

// errors (throw inside handlers)
notFound(what) | unauthorized() | forbidden() | conflict(msg, data?) | invalidInput(msg) | internal()

// webview
import { invoke, subscribe, isIpcInvocationError } from "@basket/ipc/client"
await invoke(channel, input, opts?)    // throws IpcInvocationError { code, data }
subscribe(event, (data) => void)       // → () => void
```

### @basket/window

```ts
mainWindow({ defaults, store?, storeKey? })   // → { save, storeKey }
openWindow({ url, defaults?, store?, storeKey? })  // → { id, close }
saveState(store, key)
restoreState(store, key)               // → WindowState | undefined

// re-exports from butter:
getWindow() | setWindow(opts) | closeWindow(id?)
maximize() | minimize() | restore()
fullscreen(enable) | setAlwaysOnTop(enable)
```

## Native shell

### @basket/menu

```ts
section(label, items) | item(label, action, { shortcut? }) | separator()
applyMenu(menu) | onMenu(action, () => void | Promise<void>)
```

### @basket/tray

```ts
createTray({ title?, tooltip?, icon?, items? })
  → { set(opts), remove(), onAction(action, fn) }
```

### @basket/dialog

```ts
await openFile(opts?) | openFiles(opts?) | saveFile(opts?) | openFolder(opts?)
await message(opts) | alert(msg, title?) | confirm(msg, title?) // → boolean
```

### @basket/notify

```ts
await notify({ title, body, subtitle? })
```

### @basket/shortcut

```ts
registerShortcut(id, "CmdOrCtrl+K" | { key, modifiers }, fn)
  → { id, unregister }
unregisterAll()
```

### @basket/protocol

```ts
parseDeepLink("myapp://open?id=1") // → { scheme, host, path, params, url }
onProtocol(scheme, (link) => …)    // → unsubscribe
onAnyProtocol((link) => …)
```

### @basket/lifecycle

```ts
onBeforeQuit(fn) | onWillQuit(fn) | onActivate(fn) | onReopen(fn)
```

### @basket/singleton

```ts
onSecondInstance((info) => …)            // info: { argv, cwd }
isLeader()                                // → true (in host code)
```

Requires `singleinstance` in `butter.yaml#plugins`.

### @basket/autolaunch

```ts
enable({ appId, displayName?, args?, exePath? })   // → void
disable(appId)                                      // → void
isEnabled(appId)                                    // → boolean
```

No butter plugin required — reimplements platform plumbing directly
(launchd plist on macOS, `.desktop` on Linux, HKCU Run on Windows).

### @basket/power

```ts
onSleep(fn) | onWake(fn) | onScreenSleep(fn) | onScreenWake(fn)
onLock(fn)  | onUnlock(fn)

idleSeconds()                            // → number   seconds since last input
listScreens()                            // → Screen[] { id, primary, scale, bounds, workArea }
```

Requires `power` in `butter.yaml#plugins`. Lock/unlock and idle/screen
queries are macOS-only at the shim layer today.

### @basket/sidecar

```ts
spawn(name, { args?, cwd?, env? })       // → Sidecar
listSidecars()                            // → string[]

// Sidecar:
//   pid, write(text), kill(signal?), exited: Promise<number | null>
//   onStdout(fn) → unsubscribe
//   onStderr(fn) → unsubscribe
```

Sidecars are declared in `butter.yaml#bundle.sidecars` and addressed by
basename. Resolves via `BUTTER_SIDECARS` (dev) or `BUTTER_SIDECARS_DIR`
(compiled bundle).

### @basket/theme

```ts
await getSystemTheme()                  // "light" | "dark"
onThemeChange((t) => …)                  // unsubscribe
createThemeManager({ initial?, onResolved? })
  → { get, resolved, set("light"|"dark"|"system"), subscribe }
```

## Data

### @basket/db

```ts
column.{text,integer,real,blob,boolean,timestamp,serial}()
  .primaryKey() | .unique() | .nullable() | .default(v)

defineTable(name, columns)             // → Table<C>
RowOf<typeof table>                    // → typed row

connect(path)                          // → DB (WAL + FK on)
migrate(db, [tables])                  // CREATE TABLE IF NOT EXISTS

from(table)
  .where((q) => q("col").equals(...))
  .select("col1", "col2")
  .order("col", "asc"|"desc")
  .limit(n).offset(n)

db.all(query) | db.one(query) | db.insert(table, partial)
db.update(table, where, patch) | db.remove(table, where)
db.query<R>(sql, ...params) | db.exec(sql, ...params)
db.close()
```

### @basket/migrate

```ts
diff(db, [tables])      // → { newTables, newColumns, removedColumns, … }
sync(db, [tables])      // applies additive diffs in a tx
run(db, [{ id, up, down? }])   // versioned, idempotent, tracked
rollback(db, migrations, to?)
status(db, migrations)
```

### @basket/fs

```ts
createScope(root)
  → { resolve, read, readBytes, readJson, write, writeJson,
      exists, remove, list, ensureDir }

createRecents({ store, key?, limit? })
  → { list, add, remove, clear }
```

### @basket/cache

```ts
memoryCache({ defaultTtlMs? }) | diskCache({ app, file?, defaultTtlMs? })
  → { get<T>(k), set(k, v, ttl?), has(k), delete(k), clear(),
      aside<T>(k, loader, ttl?) }
```

### @basket/logger

```ts
createLogger({ app, file?, maxSize?, keep?, level?, console? })
  → { debug, info, warn, error, child(bindings), flush, path }
```

## Network / Auth / AI

### @basket/request

```ts
createClient({ baseURL?, headers?, timeout?, retries?, retryDelay?, retryOn?,
              requestInterceptors?, responseInterceptors? })
  → { request, get, post, put, patch, delete }
isRequestError(e)   // type guard
```

### @basket/secrets

```ts
createVault(service) → { get, set, delete, getJson, setJson }
setSecret(service, key, value) | getSecret(service, key) | deleteSecret(service, key)
```

### @basket/auth

```ts
hashPassword(pw) | verifyPassword(pw, hash)
createPkce() | randomState()
openBrowser(url)
createOAuthClient({ authUrl, tokenUrl, clientId, scopes, redirectPort?, … })
  → { start(): Promise<TokenResponse>, refresh(rt): Promise<TokenResponse> }
createSession<T>({ service, key?, vault? })
  → { get, set, clear, isSignedIn }
```

### @basket/update

```ts
await check({ url, current, headers? })
  // → { available: false, current } | { available: true, current, manifest }
await download({ url, to, sha256?, onProgress? })
parseVersion(v) | compareVersions(a, b) | isNewer(remote, current)
```

### @basket/ai

```ts
openai({ apiKey, baseURL?, defaultModel?, defaultEmbedModel? }) | anthropic(...) | ollama(...)
  → Provider {
      chat({ messages, system?, temperature?, maxTokens?, signal? }),
      chatStream(...): AsyncIterable<{ delta, done }>,
      embed?({ input, model? })
    }
```

### @basket/mcp

```ts
createMcpServer({ name, version })
  .tool({ name, description?, inputSchema?, handler })
  .resource({ uri, name?, mimeType?, handler })
  .serve()   // stdio
```

## UI

### @basket/ui

```tsx
import { BasketProvider } from "@basket/ui/provider"
import { Titlebar } from "@basket/ui/titlebar"
import { SidebarLayout } from "@basket/ui/sidebar"
import { Palette, usePaletteShortcut } from "@basket/ui/palette"
import { Toaster, toast } from "@basket/ui/toast"

<BasketProvider app={{ name }} theme="auto">
  <Titlebar><Titlebar.Title>App</Titlebar.Title></Titlebar>
  <SidebarLayout>
    <SidebarLayout.Sidebar>…</SidebarLayout.Sidebar>
    <SidebarLayout.Detail>…</SidebarLayout.Detail>
  </SidebarLayout>
  <Palette open={open} onClose={close} commands={[…]} />
  <Toaster />
</BasketProvider>
```

## CLI

```bash
basket init <name> [--template minimal|menubar]
basket dev | build | bundle      # → butter dev|compile|bundle
basket add <pkg>...              # any package above
basket docs [target]
basket doctor
```

## Common patterns

### Wire menu shortcut → typed IPC

```ts
applyMenu([section("File", [item("New Note", "note:new", { shortcut: "CmdOrCtrl+N" })])])
onMenu("note:new", () => emit(noteCreated, db.insert(notesTable, { title: "Untitled" })))
```

### Persist window state on quit

```ts
import { onBeforeQuit } from "@basket/lifecycle"
const win = mainWindow({ defaults: { width: 1100, height: 720 }, store: settings })
onBeforeQuit(async () => { win.save(); await db.close(); await log.flush() })
```

### Cache-aside an API call

```ts
const data = await cache.aside(
  `repo:${name}`,
  () => api.get<Repo>(`/repos/${name}`).then((r) => r.data),
  10 * 60_000,
)
```

### OAuth + keychain session

```ts
const tok = await github.start()
await session.set({ token: tok.accessToken, refresh: tok.refreshToken })
```

### Validate IPC + structured errors

```ts
import { handle, pipeline, validate, notFound } from "@basket/ipc"
handle(getNote, pipeline(validate(noteIdSchema))(({ input }) => {
  const row = db.one(from(notes).where((q) => q("id").equals(input.id)))
  if (!row) throw notFound("note")
  return row
}))
```
