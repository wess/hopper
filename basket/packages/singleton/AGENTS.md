# @basket/singleton

Single-instance enforcement. Wraps butter's `singleinstance` plugin,
which uses a per-app lockfile + TCP-loopback signaling to make sure
only one copy of the app is running at a time. Second launches forward
their argv + cwd to the leader and exit.

> Requires `singleinstance` listed in `butter.yaml#plugins`. Without
> it, no event is ever emitted and `onSecondInstance` is a no-op.

## Exports

- `onSecondInstance(handler)` → unsubscribe — fires when another
  launch of this app forwards its argv to us.
- `isLeader()` → `true` — by the time host code is running, the plugin
  has already decided we're the leader; second launches exit before
  reaching host code.

```ts
type SecondInstanceInfo = {
  readonly argv: readonly string[]
  readonly cwd: string
}
```

## Usage

```ts
import { onSecondInstance } from "@basket/singleton"
import { mainWindow } from "@basket/window"

const win = mainWindow({ defaults: { width: 1100, height: 700 } })

onSecondInstance((info) => {
  // Raise our window — the user double-clicked the icon.
  // Optionally consume forwarded args (file paths, deep links, ...)
  if (info.argv[0]?.startsWith("myapp://")) {
    handleDeepLink(info.argv[0])
  }
})
```

## App identity

The plugin keys its lockfile by `BUTTER_APP_ID`, which butter derives
from `bundle.identifier` (preferred) or `window.title`. Set
`bundle.identifier` explicitly in `butter.yaml` so two apps with the
same title don't share a lock.

## Depends on

- `butter` (peer) — for `on()` and the `singleinstance` plugin.
