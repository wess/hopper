# @basket/tray

System tray / menubar icons. Wraps butter's tray IPC.

## Exports

- `createTray(opts)` → `TrayHandle` — install a tray icon and wire its menu

## Types

```ts
type TrayItem =
  | { label: string; action: string }
  | { separator: true }

type TrayOptions = {
  title?: string      // text shown next to the icon (macOS menubar)
  tooltip?: string    // hover tooltip
  icon?: string       // path to icon file
  items?: readonly TrayItem[]
}

type TrayHandle = {
  set: (opts: TrayOptions) => void               // replace icon/menu
  remove: () => void                              // remove from tray
  onAction: (action: string, handler: () => void) => void
}
```

## Usage

```ts
import { createTray } from "@basket/tray"

const tray = createTray({
  title: "Notes",
  tooltip: "Notes",
  icon: "./assets/tray.png",
  items: [
    { label: "Open", action: "tray:open" },
    { label: "New Note", action: "note:new" },
    { separator: true },
    { label: "Quit", action: "quit" },
  ],
})

tray.onAction("tray:open", () => {
  // bring main window to front
})

tray.onAction("quit", () => process.exit(0))
```

When a tray menu item is clicked, butter delivers the `action` string as
a regular IPC event — `onAction` is a thin wrapper over butter's `on()`.
For menubar-only apps (no main window), pair this with the `menubar`
template.

## Plugin requirement

`createTray` talks to the native shim via butter's runtime. The shim
handles `tray:set` / `tray:remove` natively — no extra plugin is needed
in `butter.yaml` for the tray to appear. If you also want
`window.butter.tray.*` available in the webview, add `tray` to the
`plugins` list in `butter.yaml`.

## Depends on

- `butter` (peer) — uses the runtime's `tell()` channel for `tray:set` /
  `tray:remove` and butter's `on()` for action handlers.
