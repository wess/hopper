# @basket/ui

Headless-ish React desktop UI primitives. Five components, one stylesheet
injected by the provider, no extra deps beyond React (and `lucide-react`
for icons in your own code).

## Exports

| Subpath | Exports |
|---|---|
| `@basket/ui/provider` | `BasketProvider`, `useBasket`, `useTheme` |
| `@basket/ui/titlebar` | `Titlebar` (+ `Titlebar.Title`, `Titlebar.Actions`) |
| `@basket/ui/sidebar`  | `SidebarLayout` (+ `.Sidebar`, `.Detail`) |
| `@basket/ui/palette`  | `Palette`, `usePaletteShortcut` |
| `@basket/ui/toast`    | `Toaster`, `toast` |
| `@basket/ui`          | everything above |

## Provider

```tsx
import { BasketProvider, useTheme } from "@basket/ui/provider"

createRoot(document.getElementById("root")!).render(
  <BasketProvider app={{ name: "Notes", id: "io.wess.notes" }} theme="auto">
    <App />
  </BasketProvider>,
)
```

`theme` is `"light" | "dark" | "auto"` (auto follows
`prefers-color-scheme`). The provider injects the basket-ui stylesheet
and sets `data-basket-ui` + `data-basket-theme` on `<html>`. CSS
variables (`--basket-bg`, `--basket-fg`, `--basket-accent`, …) are
exposed for theming your own components.

## Titlebar

```tsx
import { Titlebar } from "@basket/ui/titlebar"
import { Maximize2, X } from "lucide-react"

<Titlebar>
  <Titlebar.Title>My App</Titlebar.Title>
  <Titlebar.Actions>
    <button><Maximize2 size={14} /></button>
    <button><X size={14} /></button>
  </Titlebar.Actions>
</Titlebar>
```

Drag region is set on the bar; nested `Titlebar.Actions` is excluded.
`trafficLights` (default `true`) adds a 72-px spacer on the left for
macOS traffic-light buttons.

## SidebarLayout

```tsx
import { SidebarLayout } from "@basket/ui/sidebar"

<SidebarLayout width={280}>
  <SidebarLayout.Sidebar>
    {/* list of notes */}
  </SidebarLayout.Sidebar>
  <SidebarLayout.Detail>
    {/* editor */}
  </SidebarLayout.Detail>
</SidebarLayout>
```

CSS grid with a configurable sidebar width.

## Palette (⌘K)

```tsx
import { Palette, usePaletteShortcut, type PaletteCommand } from "@basket/ui/palette"
import { useState } from "react"

const App = () => {
  const [open, setOpen] = useState(false)
  usePaletteShortcut(() => setOpen((v) => !v))   // default ⌘K / Ctrl+K

  const commands: PaletteCommand[] = [
    { id: "new",   label: "New Note",     hint: "⌘N", action: () => createNote() },
    { id: "search", label: "Search…",     hint: "⌘F", action: () => focusSearch() },
    { id: "theme", label: "Toggle Theme", action: () => toggleTheme() },
  ]

  return (
    <>
      <Palette open={open} onClose={() => setOpen(false)} commands={commands} />
      {/* … */}
    </>
  )
}
```

Includes built-in fuzzy filtering (prefix > substring > subsequence),
arrow-key navigation, Enter to run, Esc / backdrop to close.

## Toast

```tsx
import { Toaster, toast } from "@basket/ui/toast"

const App = () => (
  <>
    <Toaster />
    <button onClick={() => toast.success("Saved!")}>Save</button>
    <button onClick={() => toast.error("Failed", { description: "Check the network." })}>Sync</button>
  </>
)
```

Mount `<Toaster />` once near the root. `toast()` (default `info`),
`toast.success`, `toast.error`, `toast.info` push entries; auto-dismiss
after `duration` ms (default 4000). Returns an id you can pass to
`toast.dismiss(id)`.

## Styling

The provider injects one stylesheet. Hooks for customization:

- Override CSS variables under `:root[data-basket-ui]` or any wrapper
  with that attribute
- Wrap children of `Titlebar.Actions` in your own `button` styling — basket
  ships zero button styles by design

## Depends on

- `react` ^19, `react-dom` ^19 (peer)
- `lucide-react` (peer, for *your* icons — basket UI itself uses none)
