# IPC Guide

Everything you need to know about `@basket/ipc`.

## Mental model

- **Channel** — request/response. Webview calls, host responds with a value.
- **Event** — one-way broadcast. Host emits, webview (or other host listeners) reacts.

Both are typed end-to-end. Both are defined in a shared file and imported on both sides.

## Defining channels

`src/shared/channels.ts`:

```ts
import { defineChannel, defineEvent } from "@basket/ipc"

export const greet      = defineChannel<{ name: string }, string>("greet")
export const listNotes  = defineChannel<void, Note[]>("notes:list")
export const createNote = defineChannel<{ title: string; body?: string }, Note>("notes:create")

export const noteCreated = defineEvent<Note>("notes:created")
export const noteDeleted = defineEvent<{ id: number }>("notes:deleted")
```

The string passed to `defineChannel` is the wire name. Keep them
namespaced (`notes:create`, `auth:signin`) — it shows up in butter's
console output and makes IPC traces readable.

`void` as the input type means "no payload"; call it as `invoke(listNotes, undefined as unknown as void)`.

## Host handler — basic

```ts
import { handle, emit } from "@basket/ipc"
import { greet, createNote, noteCreated } from "../shared/channels"

handle(greet, ({ name }) => `Hello, ${name}!`)

handle(createNote, ({ title, body }) => {
  const note = db.insert(notesTable, { title, body: body ?? "" })
  emit(noteCreated, note)
  return note
})
```

The handler can be sync or async. Return value is automatically serialized to the webview.

## Webview caller

```ts
import { invoke, subscribe } from "@basket/ipc/client"
import { greet, noteCreated } from "../shared/channels"

const reply = await invoke(greet, { name: "world" })   // string
//    ^? string

subscribe(noteCreated, (note) => {
  insertIntoList(note)
})
```

`subscribe()` returns an unsubscribe function. Save it if you ever need
to remove the listener (e.g. component teardown).

```ts
const off = subscribe(noteCreated, render)
// later:
off()
```

## Pipelines

Pre-handler work — validation, auth checks, logging, anything cross-cutting — composes via `pipeline(...)`:

```ts
import { handle, pipeline, validate, type Pipe } from "@basket/ipc"

const requireUnlocked: Pipe<unknown> = (ctx) => {
  if (vault.isLocked()) throw forbidden("Vault is locked")
  return ctx
}

const log: Pipe<unknown> = (ctx) => {
  logger.info("ipc", { input: ctx.input })
  return ctx
}

handle(createNote, pipeline(log, requireUnlocked, validate(noteSchema))(async ({ input, assigns }) => {
  // input is now typed *and* validated
  return db.insert(notesTable, input)
}))
```

A `Pipe<I, A>` receives `Ctx<I, A>` (input + assigns) and returns the next `Ctx`. Throw inside a pipe to short-circuit — the thrown error propagates to the webview as a typed `IpcInvocationError`.

### Validation

`validate(schema)` runs `schema.parse(ctx.input)` and assigns the parsed
value back. Any object with `.parse(unknown) → T` works — Zod, Yup, plain functions:

```ts
const noteSchema = {
  parse: (v: unknown) => {
    if (!v || typeof v !== "object") throw new Error("expected object")
    const o = v as Record<string, unknown>
    if (typeof o.title !== "string") throw new Error("title required")
    return { title: o.title, body: typeof o.body === "string" ? o.body : "" }
  },
}

handle(createNote, pipeline(validate(noteSchema))(({ input }) => {
  // input is { title: string; body: string }
}))
```

Validators throw a plain `Error`; basket re-wraps it as `internal` over the boundary unless you throw `invalidInput()` yourself.

### Assigns

Use `assign()` to add typed state to the context that downstream pipes / handler can read:

```ts
import { assign } from "@basket/ipc"

const loadUser: Pipe<unknown> = async (ctx) => {
  const user = await getCurrentUser()
  if (!user) throw unauthorized()
  return assign(ctx, "user", user)
}

handle(createNote, pipeline(loadUser, validate(schema))(({ input, assigns }) => {
  const userId = (assigns.user as User).id
  return db.insert(notesTable, { ...input, ownerId: userId })
}))
```

## Errors

Throw structured errors with the factory helpers:

```ts
import { notFound, unauthorized, forbidden, conflict, invalidInput, internal, ipcError } from "@basket/ipc"

handle(deleteNote, ({ id }) => {
  const note = db.one(from(notes).where((q) => q("id").equals(id)))
  if (!note) throw notFound("note")
  if (note.ownerId !== currentUser.id) throw forbidden("Not your note")
  db.remove(notes, { id })
  return { id }
})
```

Built-in codes:

| Factory | Code | Use for |
|---|---|---|
| `notFound(what)` | `not_found` | Missing resource |
| `unauthorized(msg?)` | `unauthorized` | Not signed in |
| `forbidden(msg?)` | `forbidden` | Signed in but no access |
| `conflict(msg, data?)` | `conflict` | Unique constraint, race condition |
| `invalidInput(msg, data?)` | `invalid_input` | Validation failed |
| `internal(msg?)` | `internal` | Catch-all |
| `ipcError(code, msg, data?)` | custom | Anything else |

On the webview side, errors land as `Error` with `.code` and `.data`:

```ts
import { invoke, isIpcInvocationError } from "@basket/ipc/client"

try {
  await invoke(deleteNote, { id })
} catch (e) {
  if (isIpcInvocationError(e)) {
    if (e.code === "not_found")    toast.error("Already deleted")
    else if (e.code === "forbidden") toast.error("Not your note")
    else throw e
  } else {
    throw e
  }
}
```

## Events

Events are fire-and-forget broadcasts. Host → webview:

```ts
emit(noteCreated, note)
emit(noteDeleted, { id })
```

In the webview:

```ts
subscribe(noteCreated, (note) => /* re-render list */)
subscribe(noteDeleted, ({ id }) => /* remove from list */)
```

Multiple subscribers are fine. Order is the order of subscription.

Webview → host events are rare but possible via `broadcast()`:

```ts
import { broadcast } from "@basket/ipc/client"
broadcast(userTyped, { at: Date.now() })
```

The host receives them via `listen()`:

```ts
import { listen } from "@basket/ipc"
listen(userTyped, ({ at }) => /* … */)
```

## Patterns

### CRUD with optimistic UI

```ts
// Webview
const note = { id: nextId(), title: "Untitled", body: "" }
upsertLocally(note)              // show immediately
try {
  const created = await invoke(createNote, note)
  reconcile(created)             // overwrite with server-assigned fields
} catch (e) {
  removeLocally(note.id)
  toast.error("Couldn't create note")
}
```

### Long-running tasks with progress events

```ts
// shared
export const exportNotes  = defineChannel<{ format: "md" | "json" }, { ok: true; path: string }>("notes:export")
export const exportProgress = defineEvent<{ done: number; total: number }>("notes:export:progress")

// host
handle(exportNotes, async ({ format }) => {
  const all = db.all(from(notes))
  for (const [i, note] of all.entries()) {
    await writeOne(note, format)
    emit(exportProgress, { done: i + 1, total: all.length })
  }
  return { ok: true, path: outPath }
})

// webview
subscribe(exportProgress, ({ done, total }) => setProgress(done / total))
const result = await invoke(exportNotes, { format: "md" })
```

### Authenticated channels

```ts
const requireAuth: Pipe<unknown> = async (ctx) => {
  const user = await session.get()
  if (!user) throw unauthorized()
  return assign(ctx, "user", user)
}

handle(getInbox, pipeline(requireAuth)(async ({ assigns }) => {
  const user = assigns.user as User
  return db.all(from(inbox).where((q) => q("userId").equals(user.id)))
}))
```

## Anti-patterns

- **Don't bypass the channel.** Don't reach for `butter.on()` / `butter.invoke()` directly in app code — you lose the types and the error model.
- **Don't define channels inline.** Always in `src/shared/`. If a channel is used in only one file, you have a layering problem.
- **Don't throw bare `Error`.** Throw `notFound`/`forbidden`/etc., or wrap with `ipcError(code, msg)`. The webview catches the code and routes accordingly.
- **Don't `void` an `invoke()` that might fail.** Await it, or chain `.catch()`. Unhandled rejections in the webview vanish into the console.
- **Don't put the handler logic inside the pipeline.** A pipeline is for *cross-cutting* concerns. If you only use it for one channel, just write a function and call it.

## Next

- [data.md](data.md) — how IPC handlers usually wrap `@basket/db` queries
- [auth.md](auth.md) — auth-gated channels with `requireAuth` pipes
- [`packages/ipc/AGENTS.md`](../packages/ipc/AGENTS.md) — full API surface
