# Testing Guide

Patterns for testing basket apps with `bun test`.

## What's in `bun test`

Bun ships a test runner that's API-compatible with Jest:

```ts
import { describe, expect, test, beforeEach, afterEach } from "bun:test"
```

It's fast (the full basket suite runs in ~250ms across ~100 tests). Use
it for unit tests, integration tests against `bun:sqlite`, and HTTP
tests against `Bun.serve`. For browser/webview tests, see [E2E](#e2e--webview-testing) below.

## Where tests live

Per-package:

```
packages/<name>/test/
  foo.test.ts
  bar.test.ts
```

Run all packages:

```bash
bun test
```

Run a single file or pattern:

```bash
bun test packages/db
bun test packages/db/test/db.test.ts
```

## Unit tests — pure functions

Most of basket is pure. Test directly:

```ts
import { describe, expect, test } from "bun:test"
import { compareVersions, isNewer } from "@basket/update"

describe("compareVersions", () => {
  test("equality", () => {
    expect(compareVersions("1.0.0", "1.0.0")).toBe(0)
  })
  test("major bump", () => {
    expect(isNewer("2.0.0", "1.99.99")).toBe(true)
  })
})
```

## Database tests — in-memory SQLite

`bun:sqlite` supports `:memory:` — fresh db per test, no I/O, fast:

```ts
import { beforeEach, afterEach, describe, expect, test } from "bun:test"
import { column, connect, defineTable, from, type DB } from "@basket/db"
import { sync } from "@basket/migrate"

const notes = defineTable("notes", {
  id: column.serial().primaryKey(),
  title: column.text(),
})

let db: DB

beforeEach(() => {
  db = connect(":memory:")
  sync(db, [notes])
})
afterEach(() => db.close())

test("insert + all", () => {
  db.insert(notes, { title: "hi" })
  const all = db.all(from(notes))
  expect(all).toHaveLength(1)
  expect(all[0]?.title).toBe("hi")
})
```

## Store tests — `memoryStore`

For anything that takes a `Store`, swap in `memoryStore()` for tests:

```ts
import { memoryStore } from "@basket/store"
import { createRecents } from "@basket/fs"

test("recents LRU", () => {
  const r = createRecents({ store: memoryStore(), limit: 3 })
  r.add("/a"); r.add("/b"); r.add("/c"); r.add("/a")
  expect(r.list()).toEqual(["/a", "/c", "/b"])
})
```

## HTTP tests — `Bun.serve`

For testing `@basket/request` clients (or anything that hits an HTTP API), spin up a real server in `beforeAll`:

```ts
import { afterAll, beforeAll, describe, expect, test } from "bun:test"
import { createClient } from "@basket/request"

let server: ReturnType<typeof Bun.serve>
let baseURL: string

beforeAll(() => {
  server = Bun.serve({
    port: 0,                               // random port
    fetch(req) {
      const url = new URL(req.url)
      if (url.pathname === "/echo") return Response.json({ method: req.method })
      return new Response("not found", { status: 404 })
    },
  })
  baseURL = `http://localhost:${server.port}`
})
afterAll(() => server.stop(true))

test("echoes method", async () => {
  const api = createClient({ baseURL })
  const { data } = await api.get<{ method: string }>("/echo")
  expect(data.method).toBe("GET")
})
```

This is how `@basket/request`'s own tests verify retry, abort, and interceptor behaviour.

## IPC handler tests

Handlers are just functions wrapped by `handle()`. Test the function directly without involving butter:

```ts
// src/host/handlers/notes.ts
export const listHandler = (db: DB) => () => db.all(from(notes))
export const createHandler = (db: DB) => (input: { title: string }) =>
  db.insert(notes, input)
```

```ts
// test/handlers/notes.test.ts
import { createHandler, listHandler } from "../../src/host/handlers/notes"

test("create + list", () => {
  const create = createHandler(db)
  const list = listHandler(db)
  create({ title: "Hi" })
  expect(list()).toHaveLength(1)
})

// then in src/host/index.ts:
handle(createNote, createHandler(db))
handle(listNotes,  listHandler(db))
```

The pattern: factor the business logic out of the IPC wiring. Test the
logic; trust butter to deliver the message.

## Pipeline tests

`@basket/ipc`'s `pipeline()` returns a regular function — call it directly:

```ts
import { pipeline, validate } from "@basket/ipc"

test("validate + handler composes", async () => {
  const schema = {
    parse: (v: unknown) => {
      if (typeof v !== "string") throw new Error("expected string")
      return v.trim()
    },
  }
  const handler = pipeline<string>(validate(schema))(({ input }) => input.length)
  expect(await handler("  hi  ")).toBe(2)
})
```

For pipes that throw, assert the rejection:

```ts
import { unauthorized } from "@basket/ipc"

const requireAuth = (ctx: any) => {
  throw unauthorized()
}

test("rejects unauthorized", async () => {
  const handler = pipeline(requireAuth)(() => "ok")
  await expect(handler(undefined as never)).rejects.toThrow(/Unauthorized/)
})
```

## E2E / webview testing

Bun-only tests can't drive the native webview. Options:

1. **Skip it.** Most logic lives host-side. Test the host thoroughly; rely on manual smoke-testing for the webview.
2. **Playwright with the dev server.** `butter dev` ships a debug WebSocket — Playwright can attach. Heavy setup.
3. **Headless browser against a stand-in HTTP server.** Spin up the webview HTML against Vitest + jsdom or Playwright. Doesn't catch native quirks (file dialogs, menubar) but covers React component behavior.

For a small app, do #1 plus a one-page checklist in your release docs:

- [ ] Launch the app
- [ ] Sign in (if applicable)
- [ ] Create a [thing]
- [ ] Edit a [thing]
- [ ] Delete a [thing]
- [ ] Quit and relaunch, state restored
- [ ] Theme toggle works
- [ ] ⌘K palette works
- [ ] File dialogs open

## Mocks — minimize them

Basket's design avoids the "I need a mock for X" problem:

- `memoryStore()` instead of mocking `Store`
- `:memory:` SQLite instead of mocking `DB`
- Real `Bun.serve` instead of mocking `fetch`
- `vault: createVault(...)` accepts a `Vault`, so for tests pass an
  in-memory implementation of the same shape

If a function you're writing is hard to test without mocks, factor out
the side effect:

```ts
// ❌ hard to test
const saveDraft = async (note: Note) => {
  const path = `${paths({ name: "Notes" }).data}/draft.json`
  await Bun.write(path, JSON.stringify(note))
}

// ✅ injectable
const saveDraft = (writer: (data: unknown) => Promise<void>) => (note: Note) =>
  writer(note)
```

Then in production:

```ts
const writer = async (data: unknown) => {
  const path = `${paths({ name: "Notes" }).data}/draft.json`
  await Bun.write(path, JSON.stringify(data))
}
const save = saveDraft(writer)
```

And in test:

```ts
let captured: unknown
const writer = async (data: unknown) => { captured = data }
const save = saveDraft(writer)
await save({ id: 1, title: "x" })
expect(captured).toEqual({ id: 1, title: "x" })
```

## Snapshots

Bun supports inline snapshots:

```ts
import { test, expect } from "bun:test"
import { diff } from "@basket/migrate"

test("schema diff stable shape", () => {
  const result = diff(db, [notes])
  expect(result).toMatchInlineSnapshot(`
    {
      "newColumns": [],
      "newTables": [],
      "removedColumns": [],
      "removedTables": [],
      "typeChanges": [],
    }
  `)
})
```

Run `bun test --update-snapshots` to refresh.

## Coverage

Bun has built-in coverage:

```bash
bun test --coverage
```

Outputs a percentage per file. Useful for spotting untested code paths.

## CI

Minimal GitHub Actions:

```yaml
name: ci
on: [push, pull_request]
jobs:
  test:
    runs-on: ${{ matrix.os }}
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: oven-sh/setup-bun@v2
      - run: bun install
      - run: bun run check
      - run: bun test
      - run: bunx tsc --noEmit
```

`bunx tsc --noEmit` is non-negotiable — Bun's test runner doesn't fail on type errors.

## Anti-patterns

- **Don't test against the real keychain.** Use `memoryStore` + a fake
  `Vault` impl. Real `@basket/secrets` calls spawn `security` / `secret-tool` and pollute the user's keychain.
- **Don't test against a real OAuth provider.** Mock `tokenUrl` with `Bun.serve`. The OAuth flow's surface area is small; the security details are in the SDK, not your code.
- **Don't rely on snapshot tests as your only assertion.** Snapshots catch regressions but don't express intent. Pair with explicit `expect()`.
- **Don't keep slow tests.** A fast suite is one you actually run. If a test takes > 100 ms, profile and fix or split.

## Next

- [`packages/<name>/test/`](../packages) — every package has tests; use them as references
- [troubleshooting.md](troubleshooting.md) — common test issues
