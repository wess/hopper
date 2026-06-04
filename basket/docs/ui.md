# UI Guide

`@basket/ui` is a small set of React 19 desktop primitives that match the
basket philosophy: minimal deps, functional, themable via CSS variables.

> Webview-only. Don't import from the host bundle.

## Five components

| Component | Subpath | What it does |
|---|---|---|
| `BasketProvider` | `@basket/ui/provider` | Root context — theme, app info, injects CSS |
| `Titlebar`       | `@basket/ui/titlebar` | Native-feel titlebar with drag region |
| `SidebarLayout`  | `@basket/ui/sidebar`  | Sidebar + main grid |
| `Palette`        | `@basket/ui/palette`  | ⌘K command palette with fuzzy search |
| `Toaster`        | `@basket/ui/toast`    | Toast notifications + imperative `toast()` API |

You can import everything from `@basket/ui` or pick per-subpath for tree-shaking.

## Setup

In your webview entry:

```tsx
// src/app/main.tsx
import { createRoot } from "react-dom/client"
import { BasketProvider } from "@basket/ui/provider"
import { App } from "./app"

createRoot(document.getElementById("root")!).render(
  <BasketProvider app={{ name: "Notes", id: "io.wess.notes" }} theme="auto">
    <App />
  </BasketProvider>,
)
```

`theme` is `"light" | "dark" | "auto"`. `"auto"` follows
`prefers-color-scheme` automatically. The provider injects a stylesheet
and sets `data-basket-ui` / `data-basket-theme` on `<html>`.

## Theming

CSS variables under `:root[data-basket-ui]`:

```
--basket-bg              page background
--basket-bg-elevated     cards, panels, titlebar
--basket-bg-overlay      hover backgrounds
--basket-fg              primary text
--basket-fg-muted        secondary text
--basket-border          borders
--basket-selected        selected list item bg
--basket-accent          primary accent (button bg, focus ring)
--basket-radius          border-radius
--basket-shadow          modal / dropdown shadow
--basket-font            font family
```

Override per-app:

```css
:root[data-basket-ui] {
  --basket-accent: #ff6b35;
  --basket-radius: 8px;
}
```

Or per-theme:

```css
:root[data-basket-ui][data-basket-theme="dark"] {
  --basket-bg: #0a0a0c;
}
```

## Titlebar

```tsx
import { Titlebar } from "@basket/ui/titlebar"
import { Search, Settings } from "lucide-react"

<Titlebar>
  <Titlebar.Title>Notes</Titlebar.Title>
  <Titlebar.Actions>
    <button><Search size={14} /></button>
    <button><Settings size={14} /></button>
  </Titlebar.Actions>
</Titlebar>
```

The bar has `-webkit-app-region: drag` so dragging it moves the window.
`Titlebar.Actions` excludes its children from the drag region so clicks
land. `trafficLights={true}` (default) adds a 72-px spacer for macOS
traffic-light buttons.

For a frameless butter window, set `frameless: true` in `butter.yaml` and use `Titlebar` to draw the chrome yourself.

## SidebarLayout

```tsx
import { SidebarLayout } from "@basket/ui/sidebar"

<SidebarLayout width={280}>
  <SidebarLayout.Sidebar>
    <NoteList />
  </SidebarLayout.Sidebar>
  <SidebarLayout.Detail>
    <NoteEditor />
  </SidebarLayout.Detail>
</SidebarLayout>
```

A two-column CSS grid. `width` controls the sidebar column. Both panels
overflow individually — you can scroll the list without scrolling the
editor.

## Palette (⌘K)

```tsx
import { useState } from "react"
import { Palette, usePaletteShortcut, type PaletteCommand } from "@basket/ui/palette"
import { Plus, Search, Moon, Sun, FileDown } from "lucide-react"

const App = () => {
  const [open, setOpen] = useState(false)
  usePaletteShortcut(() => setOpen((v) => !v))   // default ⌘K / Ctrl+K

  const commands: PaletteCommand[] = [
    { id: "new",      label: "New Note",        hint: "⌘N", icon: <Plus size={14}/>,    action: createNote },
    { id: "search",   label: "Search…",         hint: "⌘F", icon: <Search size={14}/>,  action: focusSearch },
    { id: "theme",    label: "Toggle Theme",                icon: <Moon size={14}/>,    action: toggleTheme,
      keywords: ["dark", "light", "mode"] },
    { id: "export",   label: "Export as Markdown",          icon: <FileDown size={14}/>, action: exportMd },
  ]

  return (
    <>
      <Palette
        open={open}
        onClose={() => setOpen(false)}
        commands={commands}
        placeholder="What do you want to do?"
      />
      {/* … */}
    </>
  )
}
```

Filtering:

- **Prefix match** wins (score 3) — typing "new" promotes "New Note"
- **Substring match** next (score 2) — "exp" matches "Export"
- **Fuzzy subsequence** last (score 1) — "txm" matches "Toggle Theme"
- `keywords` are included in the searched text

Navigation:

- ↑ / ↓ to move selection
- ⏎ to run
- Esc / backdrop to close

`onClose()` is called *before* `action()` runs, so the palette is gone
before your handler executes.

## Toast

Mount the `<Toaster />` once near the root, then call `toast` imperatively from anywhere:

```tsx
import { Toaster, toast } from "@basket/ui/toast"

<>
  <Toaster />
  <button onClick={() => toast.success("Saved!")}>Save</button>
  <button onClick={() => toast.error("Failed", { description: "Check your connection." })}>Sync</button>
</>
```

Variants: `toast(title, opts?)` (info), `toast.info`, `toast.success`, `toast.error`. Each returns an id:

```ts
const id = toast.info("Working…", { duration: 0 })   // 0 = sticky
// later
toast.dismiss(id)
```

`duration` is in ms (default 4000). Pass 0 to keep until dismissed.

## Composing them

A typical app shell:

```tsx
<BasketProvider app={app}>
  <Titlebar>
    <Titlebar.Title>{title}</Titlebar.Title>
    <Titlebar.Actions>
      <button onClick={() => setPalette(true)}><Search size={14}/></button>
    </Titlebar.Actions>
  </Titlebar>

  <SidebarLayout>
    <SidebarLayout.Sidebar>
      <NoteList />
    </SidebarLayout.Sidebar>
    <SidebarLayout.Detail>
      <NoteEditor />
    </SidebarLayout.Detail>
  </SidebarLayout>

  <Palette open={palette} onClose={() => setPalette(false)} commands={commands} />
  <Toaster />
</BasketProvider>
```

CSS-wise, `BasketProvider` injects the stylesheet but doesn't constrain
layout. Set `html, body { height: 100%; margin: 0; }` in your own CSS,
and let `SidebarLayout` fill the available space.

## Hooks

`useBasket()` — context access:

```tsx
import { useBasket, useTheme } from "@basket/ui/provider"

const { app } = useBasket()
const { resolved, setTheme } = useTheme()
```

`usePaletteShortcut(toggle, key?)` — register a ⌘K (or any chord):

```tsx
usePaletteShortcut(() => setOpen((v) => !v))            // ⌘K / Ctrl+K
usePaletteShortcut(() => setOpen((v) => !v), "p")        // ⌘P / Ctrl+P
```

## Why no styled buttons or inputs?

By design. Native-feeling buttons depend heavily on platform, brand,
and density choices. Shipping a `Button` with one opinion hurts more
than helps. Style your own — basket gives you the *layouts* and *flows*
(titlebar drag, palette filtering, toast queue) where there's a clear
right answer.

If you want a more opinionated kit on top of basket, build it as
`@yourapp/ui` and consume basket's primitives. Atlas's `@atlas/ui`
likewise leans on Mantine for buttons and pickers.

## Anti-patterns

- **Don't render `<BasketProvider>` twice.** It injects styles once; nested providers re-inject and may flicker.
- **Don't use `Titlebar` without a frameless window.** macOS draws its own titlebar above yours.
- **Don't put long-running work in a palette command's `action`.** `onClose()` fires synchronously before `action()`; if the action throws or hangs, the user sees no feedback. Wrap in `toast.error()` / `toast.success()`.
- **Don't drive theme from local state.** Let the user's preference flow through `useTheme()` and persist to `@basket/store` on change.

## Next

- [`packages/ui/AGENTS.md`](../packages/ui/AGENTS.md) — full component API
- [cookbook.md](cookbook.md) — UI recipes (toolbar groups, status bar, etc.)
