# @basket/menu

Declarative builders for native app menus. Thin wrapper over butter's
`setMenu()` plus an `onMenu()` shortcut that maps menu actions to IPC
handlers.

## Exports

- `section(label, items)` → `MenuSection`
- `item(label, action, opts?)` → `MenuItem` — `opts.shortcut?: string` (e.g. `"CmdOrCtrl+S"`)
- `separator()` → `MenuItem`
- `applyMenu(menu)` — push the menu to the native shim via butter
- `onMenu(action, handler)` — register a handler for a menu action

## Types

```ts
type MenuItem =
  | { label: string; action: string; shortcut?: string }
  | { separator: true }

type MenuSection = { label: string; items: readonly MenuItem[] }
type Menu = readonly MenuSection[]
```

These are structurally identical to butter's `Menu` type — basket just
adds the builder ergonomics and the action-handler shortcut.

## Usage

```ts
import { applyMenu, item, onMenu, section, separator } from "@basket/menu"

applyMenu([
  section("File", [
    item("New Note", "note:new", { shortcut: "CmdOrCtrl+N" }),
    item("Open…", "note:open", { shortcut: "CmdOrCtrl+O" }),
    separator(),
    item("Quit", "quit", { shortcut: "CmdOrCtrl+Q" }),
  ]),
  section("Edit", [
    item("Undo", "undo", { shortcut: "CmdOrCtrl+Z" }),
    item("Redo", "redo", { shortcut: "CmdOrCtrl+Shift+Z" }),
  ]),
])

onMenu("note:new", () => {
  // create a new note
})

onMenu("quit", () => process.exit(0))
```

The `action` string on each `item()` is the IPC name. `onMenu(action, fn)`
registers a no-input handler — for parameterised actions, use
`@basket/ipc`'s `defineChannel` and `handle()` directly.

## Depends on

- `butter` (peer) — for `setMenu` and `on`
