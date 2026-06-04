# @basket/shortcut

Global (system-wide) and scoped keyboard shortcuts. Wraps butter's
`globalshortcuts` plugin.

> Requires `globalshortcuts` listed in `butter.yaml#plugins`.

## Exports

- `registerShortcut(id, shortcut, handler)` → `ShortcutHandle`
- `unregisterAll()` — release every registered shortcut
- `parseShortcut(spec)` → `Shortcut` — parse `"CmdOrCtrl+Shift+P"` style strings

## Types

```ts
type Modifier = "cmd" | "ctrl" | "alt" | "shift"

type Shortcut = {
  key: string                    // e.g. "p", "space", "f1"
  modifiers?: readonly Modifier[]
}

type ShortcutHandle = {
  id: string
  unregister: () => void
}
```

## Usage

```ts
import { registerShortcut, unregisterAll } from "@basket/shortcut"

// String form — "CmdOrCtrl" resolves per platform
const palette = registerShortcut("palette", "CmdOrCtrl+K", () => {
  emit(openPalette, undefined)
})

// Structured form
const dump = registerShortcut("dump-state", { key: "d", modifiers: ["cmd", "shift"] }, async () => {
  await writeDebugSnapshot()
})

// later
palette.unregister()

// on quit
unregisterAll()
```

## Scope

These are **system-global** shortcuts — they fire even when your app is
in the background. For window-scoped shortcuts that only fire when your
window is focused, attach a `keydown` listener in the webview instead.

## Caveats

- macOS requires Accessibility permission for some shortcuts; the OS
  prompts on first registration.
- Avoid claiming common system shortcuts (⌘Space, ⌘Tab) — registration
  succeeds but the OS shortcut wins.
- Shortcuts are released when the app exits. Always pair every
  `registerShortcut` with an `unregister()` or call `unregisterAll()`
  on `before-quit`.

## Depends on

- `butter` (peer) — `globalshortcuts` plugin must be loaded.
