# @basket/logger

File-rotating logger that writes to `paths(app).logs`. Async, queued
writes; never blocks the host. Optional console mirror for dev.

## Exports

- `createLogger(options)` → `Logger`
- `LEVELS` — `{ debug: 10, info: 20, warn: 30, error: 40 }`

## Types

```ts
type LogLevel = "debug" | "info" | "warn" | "error"

type LoggerOptions = {
  app:        AppId          // routes file under paths(app).logs
  file?:      string         // default "app.log"
  maxSize?:   number         // bytes, default 5_000_000
  keep?:      number         // rotated files to keep, default 5
  level?:     LogLevel       // default "info"
  console?:   boolean        // mirror to stdout/stderr, default false
}

type Logger = {
  path: string
  debug/info/warn/error: (message, meta?) => void
  child: (bindings: Record<string, unknown>) => Logger
  flush: () => Promise<void>
}
```

## Usage

```ts
import { createLogger } from "@basket/logger"

const log = createLogger({
  app: { name: "Notes" },
  level: "info",
  console: Bun.env.NODE_ENV !== "production",
})

log.info("starting", { version: "1.0.0" })
log.warn("rate limited", { tries: 3 })
log.error("sync failed", { url, status: 500 })

const requestLog = log.child({ requestId: "abc" })
requestLog.info("handling")    // includes { requestId: "abc" }
```

## Rotation

When the active file exceeds `maxSize`, the logger:

1. `app.log.{n}` → `app.log.{n+1}` for n = keep-1 .. 1
2. `app.log` → `app.log.1`
3. Starts a new `app.log`

Files older than `keep` are overwritten.

## Format

```
2026-05-08T12:00:00.000Z INFO  starting {"version":"1.0.0"}
```

ISO timestamp, level (left-padded to 5 chars), message, optional
JSON meta.

## Flushing

Writes are queued asynchronously. Call `log.flush()` before exit (or in
`@basket/lifecycle`'s `beforeQuit`) if you want every line on disk:

```ts
import { onBeforeQuit } from "@basket/lifecycle"
onBeforeQuit(async () => {
  await log.flush()
})
```

## Depends on

- `@basket/config` — `paths(app).logs`
- `Bun.write`, `node:fs/promises` `rename`/`mkdir`/`stat`
