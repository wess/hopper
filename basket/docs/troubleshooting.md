# Troubleshooting

Common errors and fixes, grouped by symptom.

## Build / install

### `Cannot find module '@basket/*'` in a fresh project

Your `package.json` needs to declare `workspaces` *and* declare the
package as a dep:

```json
{
  "workspaces": ["basket/packages/*"],
  "dependencies": {
    "@basket/config": "workspace:*"
  }
}
```

Then `bun install`. The `tsconfig.json#paths` in basket already resolves the import.

### `bun install` complains about missing `butter`

Add it as an explicit dep:

```json
{
  "dependencies": {
    "butter": "npm:butterframework@latest"
  }
}
```

`butter` is the npm name; `butterframework` is the package on npm. Aliased on install.

### TypeScript can't see `@basket/*` paths

Make sure your project's `tsconfig.json` extends or duplicates basket's `paths`:

```json
{
  "extends": "./basket/tsconfig.json"
}
```

Or copy the relevant aliases into your own tsconfig.

### `Butter runtime not initialized`

You imported a basket package (e.g. `@basket/tray`, `@basket/window`)
before butter set up `globalThis.__butterRuntime`. Make sure butter is
the first import in `src/host/index.ts`:

```ts
import "butter"   // ensures runtime is set up
// then everything else
```

In practice butter's CLI handles this. The error usually means you're
running the host script directly (`bun src/host/index.ts`) instead of
through `butter dev`.

## Window / IPC

### Webview shows "Failed to load resource"

The `butter.yaml#build.entry` path is wrong, or the file doesn't exist
relative to your project root. Run `ls src/app/index.html` and double-check.

### `invoke()` hangs forever

The host handler is registered after the webview's `invoke()` fired. In
`src/host/index.ts`, register all `handle()` calls *before* setting up
the window:

```ts
// ✅
handle(greet, …)
mainWindow({ … })

// ❌
mainWindow({ … })          // webview loads, may call invoke immediately
handle(greet, …)           // too late
```

Or — easier — make the webview wait until DOMContentLoaded before its first invoke.

### Webview can't see a host event

`subscribe()` returns an unsubscribe function. If you accidentally call
it (e.g. by destructuring), you immediately unsubscribe:

```ts
// ❌ — calls the returned unsubscribe right away
const { off } = subscribe(noteCreated, render)

// ✅
const off = subscribe(noteCreated, render)
```

### `invoke()` throws "code: internal"

Your handler threw a plain `Error`. Basket wraps non-tagged errors with `code: "internal"` over the wire. Throw `notFound()`, `invalidInput()`, etc., from `@basket/ipc` instead:

```ts
import { notFound, invalidInput } from "@basket/ipc"

handle(getNote, ({ id }) => {
  if (!validId(id)) throw invalidInput("Bad id")
  const row = db.one(from(notes).where((q) => q("id").equals(id)))
  if (!row) throw notFound("note")
  return row
})
```

## SQLite / data

### `SQLITE_CANTOPEN: unable to open database file`

The parent directory doesn't exist. Call `ensurePaths()` before `connect()`:

```ts
const p = await ensurePaths({ name: "Notes" })
const db = connect(`${p.data}/notes.db`)
```

### `Cannot add a NOT NULL column with default value NULL`

You added a non-nullable column without a default to an existing table.
Either:

- Make it nullable: `column.text().nullable()`
- Give it a default: `column.text().default("")`
- Or write a migration that does ADD COLUMN (nullable), backfill, then create a CHECK constraint.

`sync()` will warn about this. Don't fight it; pick one of the three.

### Migration applied but row format changed

You rewrote a column from `INTEGER` to `TEXT` (or similar). SQLite
doesn't actually re-encode existing rows — they stay in their old
representation. To force re-encoding you have to recreate the table:

```ts
{
  id: "2026-05-01_retype_score",
  up: (db) => {
    db.raw.exec(`
      CREATE TABLE notes_new (id INTEGER PRIMARY KEY, score TEXT);
      INSERT INTO notes_new (id, score) SELECT id, CAST(score AS TEXT) FROM notes;
      DROP TABLE notes;
      ALTER TABLE notes_new RENAME TO notes;
    `)
  },
}
```

### `database is locked`

WAL mode (which `connect()` enables) prevents this for readers vs
writers, but multiple writers from different processes can still
collide. Either:

- Use one writer process (the host), and have other processes (MCP, dev scripts) read only
- Wrap conflicting writes in `db.raw.transaction()` — SQLite serializes inside a transaction
- Increase the busy-timeout: `db.raw.exec("PRAGMA busy_timeout = 5000")`

## Keychain

### `security: SecKeychainSearchCopyNext: The specified item could not be found`

Expected. `getSecret` returns `undefined` for missing keys. If you see
this in stderr during normal use, it means a `Bun.$` call leaked stderr.
Wrap it: `.quiet().nothrow()` (already done in `@basket/secrets`).

### Windows `getSecret` always returns `undefined`

Known. `cmdkey` doesn't expose stored passwords. Options:

- Ship a native helper (e.g. a small C++ tool using `CredRead`) and call it from `@basket/secrets`'s `winGet`
- Use DPAPI-encrypted files on Windows instead (write to `paths(app).config/secrets.enc`)
- Switch to a cross-platform native module like `keytar` — but that's a native binary, fragile across Bun versions

### Linux: `Cannot autolaunch D-Bus without X11 $DISPLAY`

`secret-tool` needs a keyring daemon and a D-Bus session. Common on
headless CI. Fixes:

- Run under a real desktop session (don't test on a no-display CI runner)
- Or mock `@basket/secrets` in CI: replace `createVault` with an in-memory `Vault` impl

## OAuth

### "Port already in use" on `oauth.start()`

A previous sign-in attempt left the loopback server running, or another
app uses port 53682. Either change the port:

```ts
createOAuthClient({ …, redirectPort: 5174 })
```

…or kill the stray process: `lsof -i :53682` then `kill <pid>`.

The OAuth client shuts down its server in `queueMicrotask` after
resolve/reject — if the process crashes mid-flow, the port leaks until
process exit.

### "state mismatch"

Possible causes:

- The auth provider didn't preserve the `state` param. Some providers strip it; check their docs.
- Two concurrent `start()` calls. The OAuth client uses a single port; concurrent calls race.

### `redirect_uri_mismatch` from the provider

The provider's app config doesn't include your loopback URL. Register
`http://127.0.0.1:53682/callback` (or whatever port you used) in the
provider's OAuth app settings. Most providers allow multiple redirect
URIs.

## UI

### Stylesheet doesn't load / theme stuck on light

`<BasketProvider>` injects the stylesheet inline. If it's not rendering, check that:

- `<BasketProvider>` wraps your app, not just a leaf
- React rendered (no error in the console blocking it)
- You're not setting `data-basket-theme` manually elsewhere — let the provider do it

### Palette ⌘K not opening

`usePaletteShortcut` listens on `window.keydown`. If your input has a
keydown handler that `stopPropagation()`s, the global listener never
fires. Either:

- Don't stop propagation on `keydown` from inputs
- Or wire your own shortcut explicitly and call `setOpen(true)`

### Toast appears off-screen

Add `position: relative` (or anything other than `static`) to a parent
that owns the layout. The toaster uses `position: fixed` relative to
the viewport, which can be wrong inside iframes or weird CSS contexts.

## Performance

### Webview is slow on a particular Linux distro

WebKitGTK varies in quality across distros. If users report sluggish
behavior, suggest installing the latest `webkit2gtk-4.1` package.

### `bun test` slows over time

Likely a leaked `Bun.serve` from an HTTP test that didn't call `stop()` in `afterAll`. Audit your `beforeAll` / `afterAll` pairs.

### Logger blocks the event loop

`@basket/logger` queues writes asynchronously — but if you `await
log.flush()` in a hot loop, it serializes. Don't `flush()` per-line;
flush once on shutdown.

## Distribution

### macOS: "<App> can't be opened because Apple cannot check it for malicious software"

You shipped an unsigned / unnotarized build. See [distribution.md](distribution.md#macos).

### Auto-updater says "no update" but I bumped the version

`compareVersions` is dot-separated integers, not full semver. `1.10.0` is *less than* `1.9.0` if you compare strings, but **greater** with `compareVersions`. The bug is usually in your manifest — you wrote `"version": "v1.10.0"` and your code compares without stripping the `v` prefix. `parseVersion` strips a leading `v` automatically; verify.

### Update download fails on Linux with self-signed certs

`fetch` in Bun honors the system CA bundle. If your update server uses
a self-signed cert, set `NODE_EXTRA_CA_CERTS` or use a public-CA cert.

## Still stuck?

- Run `basket doctor` — checks bun + butter
- Open the package's `AGENTS.md` — `basket docs <package>`
- Search butter's issues — many "weird native behavior" bugs are upstream
- File an issue at https://github.com/wess/basket

## Next

- [faq.md](faq.md) — questions that aren't "why doesn't X work"
- [contributing.md](contributing.md) — how to fix or add things
