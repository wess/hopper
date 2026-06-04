# @basket/ipc

Typed IPC channels and events over butter's IPC. End-to-end types, pipeline composition, runtime validation, and structured error propagation across the host ↔ webview boundary.

## Exports

### `@basket/ipc` (host side)

- `defineChannel<I, O>(name)` → `Channel<I, O>` — request/response channel
- `defineEvent<T>(name)` → `Event<T>` — host→webview broadcast event
- `handle(channel, handler, opts?)` — register a host handler; opts.input optionally validates
- `emit(event, data)` — broadcast an event to the webview
- `listen(event, fn)` — listen for a webview-broadcast event on the host
- `pipeline(...pipes)(handler)` — compose middleware around a handler
- `validate(schema)` — pipe that runs `schema.parse(ctx.input)`
- `assign(ctx, key, value)` — pure helper to add a typed assigns key
- Error factories: `ipcError`, `notFound`, `unauthorized`, `forbidden`, `conflict`, `invalidInput`, `internal`
- `encodeError(e)` / `decodeError(e)` — serialize tagged errors over IPC

### `@basket/ipc/client` (webview side)

- `invoke(channel, input, opts?)` → `Promise<O>` — call a host handler; throws `IpcInvocationError` (Error with `.code` / `.data`) for structured errors
- `subscribe(event, fn)` → `() => void` — subscribe to a host event; returns unsubscribe
- `broadcast(event, data)` — webview→host event
- `isIpcInvocationError(e)` — type guard

## Types

```ts
type Channel<I, O> = { name: string; __i?: I; __o?: O }
type Event<T> = { name: string; __t?: T }

type Ctx<I, A> = { input: I; assigns: Readonly<A> }
type Pipe<I, A> = (ctx: Ctx<I, A>) => Ctx<I, A> | Promise<Ctx<I, A>>
type Validator<T> = { parse: (v: unknown) => T }

type IpcErrorShape = { code: string; message: string; data?: unknown }
type IpcInvocationError = Error & { code: string; data?: unknown }
```

## Usage

### Shared channels

```ts
// src/shared/channels.ts
import { defineChannel, defineEvent } from "@basket/ipc"

export const greet = defineChannel<{ name: string }, string>("greet")
export const noteCreated = defineEvent<{ id: number; title: string }>("note:created")
```

### Host with pipeline + validation + structured errors

```ts
import { handle, pipeline, validate, notFound, conflict, type Ctx } from "@basket/ipc"
import { greet } from "../shared/channels"

const schema = {
  parse: (v: unknown) => {
    if (!v || typeof v !== "object" || typeof (v as { name?: unknown }).name !== "string") {
      throw new Error("name required")
    }
    return v as { name: string }
  },
}

const requireUnlocked: Pipe<unknown> = (ctx) => {
  if (!locked()) return ctx
  throw forbidden("Vault is locked")
}

handle(greet, pipeline(validate(schema), requireUnlocked)(async ({ input }) => {
  if (input.name === "banned") throw notFound("user")
  return `Hello, ${input.name}!`
}))
```

### Plain handler (no pipeline)

```ts
handle(greet, ({ name }) => `Hello, ${name}!`)
```

### Webview

```ts
import { invoke, subscribe, isIpcInvocationError } from "@basket/ipc/client"
import { greet } from "../shared/channels"

try {
  const msg = await invoke(greet, { name: "world" })
} catch (e) {
  if (isIpcInvocationError(e) && e.code === "not_found") {
    // handle not_found
  } else {
    throw e
  }
}
```

## Error model

Errors thrown from `handle()` handlers are passed through `encodeError()`,
which tags them with `__ipc__:<json>` in the message. The webview's
`invoke()` calls `decodeError()` and re-throws with `.code` / `.data`
fields exposed. Unknown throwables become `{ code: "internal", message }`.

Validators throw plain `Error`s; basket re-wraps them as `invalid_input`
when raised inside `validate()` pipe (via `encodeError`'s `internal`
fallback — for stronger semantics, throw `invalidInput("…")` yourself).

## Why share channel files

Both host and webview import the same `Channel<I, O>` definitions, so the
IPC name and payload types are defined once. Renaming a channel updates
both sides via tsserver rename. Mismatched I/O is a compile-time error.

## Depends on

- `butter` (peer) on host. The `client` subpath uses the `butter` global
  injected into the webview.
