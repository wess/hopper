# @basket/power

System power, screen-lock, idle, and display info. Wraps butter's
`power` plugin events and control roundtrips with multi-subscriber
handlers and typed queries.

> Requires `power` listed in `butter.yaml#plugins`. Events come from
> the native shim — on macOS this is `NSWorkspace` + the screen-lock
> distributed notification; Linux/Windows are stubs until the shim
> adds equivalent hooks (the JS API is portable but quiet).

## Exports — events

| Function | Fires when |
|---|---|
| `onSleep(h)` | system is about to suspend |
| `onWake(h)` | system resumed from sleep |
| `onScreenSleep(h)` | display turned off (idle / Lid closed) |
| `onScreenWake(h)` | display turned back on |
| `onLock(h)` | screen was locked |
| `onUnlock(h)` | screen was unlocked |

Each returns an unsubscribe function. Handlers may be sync or `async`;
errors are swallowed so one handler can't block the others.

## Exports — queries

- `idleSeconds()` → `Promise<number>` — seconds since the last HID
  event (mouse/keyboard) of any kind. Useful for auto-pause /
  away-status detection.
- `listScreens()` → `Promise<readonly Screen[]>` — connected displays.

```ts
type Screen = {
  readonly id: number
  readonly primary: boolean
  readonly scale: number
  readonly bounds:   { x: number; y: number; width: number; height: number }
  readonly workArea: { x: number; y: number; width: number; height: number }
}
```

`bounds` is the full display rect; `workArea` excludes the menu bar /
taskbar / dock.

## Usage

```ts
import { onLock, onUnlock, idleSeconds, listScreens } from "@basket/power"

onLock(() => pauseRecording())
onUnlock(() => resumeRecording())

// Mark user as away after 5 minutes of no input
setInterval(async () => {
  if ((await idleSeconds()) > 300) setStatus("away")
}, 30_000)

const [primary] = (await listScreens()).filter((s) => s.primary)
console.log(`Primary display: ${primary?.bounds.width}x${primary?.bounds.height}`)
```

## Depends on

- `butter` (peer) — for `on()`, `idleSeconds()`, `listScreens()` and
  the `power` plugin events.
