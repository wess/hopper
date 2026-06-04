# @basket/window

Multi-window management with size/position persistence. Wraps butter's window primitives and adds a `Store`-backed state restore.

## Exports

- `mainWindow(opts)` → `MainWindowHandle` — configure the main window with defaults + optional persistence
- `openWindow(opts)` → `WindowHandle` — create a new window with optional persistence
- `saveState(store, key)` — snapshot `getWindow()` into the store
- `restoreState(store, key)` → `WindowState | undefined` — read a previously saved snapshot
- Re-exports from `butter`: `getWindow`, `setWindow`, `closeWindow`, `maximize`, `minimize`, `restore`, `fullscreen`, `setAlwaysOnTop`

## Types

```ts
type WindowState = {
  width?: number; height?: number
  x?: number; y?: number
  title?: string
  fullscreen?: boolean; alwaysOnTop?: boolean
}

type MainWindowOptions = {
  defaults: WindowState
  store?: Store
  storeKey?: string  // default "main"
}

type MainWindowHandle = {
  save: () => void
  storeKey: string
}

type OpenWindowOptions = {
  url: string
  defaults?: Omit<CreateWindowOptions, "url">  // butter's CreateWindowOptions
  store?: Store
  storeKey?: string
}

type WindowHandle = {
  id: string
  close: () => void
}
```

## Usage

### Main window with persistence

```ts
import { createStore } from "@basket/store"
import { mainWindow } from "@basket/window"

const wins = createStore("windows", { app: { name: "Notes" } })

const main = mainWindow({
  defaults: { width: 1200, height: 800, title: "Notes" },
  store: wins,
})

// Save on shutdown — wire to butter's lifecycle plugin
process.on("beforeExit", () => main.save())
```

`mainWindow()` reads `wins.get("main")` (or your `storeKey`) and merges
the saved size/position over the defaults before calling
butter's `setWindow()`. First launch uses defaults verbatim.

### Secondary window

```ts
import { openWindow } from "@basket/window"

const editor = openWindow({
  url: "/editor.html",
  defaults: { width: 900, height: 700, title: "Editor" },
  store: wins,
  storeKey: "editor",
})

// later
editor.close()
```

### Manual save/restore

```ts
import { saveState, restoreState } from "@basket/window"

saveState(wins, "main")
const prev = restoreState(wins, "main")  // WindowState | undefined
```

## When to save

Butter does not currently surface a "window resized" callback. Default
strategy: save on shutdown via the `lifecycle` plugin's `before-quit`
event. For high fidelity, snapshot on a `setInterval` or after specific
user actions (toggling fullscreen, opening DevTools, etc.).

## Translucent windows (vibrancy / mica / acrylic)

Butter exposes a `material` window option that applies an
`NSVisualEffectView` on macOS and the DWM system backdrop on Windows
11+. The webview is set transparent so the material shows through —
make sure your app's CSS doesn't paint an opaque `body { background }`.

```yaml
# butter.yaml — main window
window:
  material: vibrancy   # vibrancy | mica | acrylic | tabbed | none
```

```ts
// Secondary windows pass it through openWindow defaults:
import { openWindow } from "@basket/window"

const palette = openWindow({
  url: "/palette.html",
  defaults: { width: 720, height: 480, frameless: true, material: "vibrancy" },
})
```

`material` is fixed at window creation — you can't toggle it at
runtime through `setWindow()`. Linux has no equivalent; the option is
a no-op there.

## Depends on

- `butter` (peer) — for `setWindow`, `createWindow`, etc.
- `@basket/store` — for the persistence layer
