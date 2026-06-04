# @basket/lifecycle

App lifecycle hooks. Wraps butter's `lifecycle` plugin events with
typed, multi-subscriber handlers.

> Requires `lifecycle` listed in `butter.yaml#plugins` to receive
> `app:activate` / `app:reopen`. `beforeQuit` also subscribes to
> `process.on("beforeExit")` as a fallback for clean exits.

## Exports

- `onBeforeQuit(handler)` → unsubscribe — app is about to quit cleanly
- `onWillQuit(handler)` → unsubscribe — SIGHUP/SIGTERM received
- `onActivate(handler)` → unsubscribe — app activated (foregrounded)
- `onReopen(handler)` → unsubscribe — Dock icon clicked (macOS) with no open windows

Each handler may be sync or `async`. Errors are swallowed so one
handler can't block the others.

## Usage

```ts
import { onBeforeQuit, onActivate } from "@basket/lifecycle"
import { mainWindow } from "@basket/window"

const win = mainWindow({ defaults: { width: 1100, height: 700 }, store: settings })

onBeforeQuit(async () => {
  win.save()
  await db.close()
  await log.flush()
})

onActivate(() => {
  // foregrounded — refresh data, ping server, etc.
})
```

## Why both `beforeQuit` and `willQuit`

- `beforeQuit` fires on a clean shutdown (user picked Quit, main process
  is about to exit). Best for flush/save work — you have time.
- `willQuit` fires on SIGHUP/SIGTERM (kill, OS shutting down). Minimal
  time. Use for last-ditch state dumps.

## Depends on

- `butter` (peer) — for the `on()` listener and lifecycle plugin events.
