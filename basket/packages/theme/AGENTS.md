# @basket/theme

System theme (dark/light) detection plus an app-level theme manager
that supports a `"system"` mode that follows OS changes live.

> Requires `theme` listed in `butter.yaml#plugins` for live system
> theme-change events. `getSystemTheme()` works without the plugin.

## Exports

- `getSystemTheme()` → `"light" | "dark"` (one-shot probe)
- `onThemeChange(fn)` → unsubscribe; fires on OS theme changes
- `createThemeManager(options?)` → app-level manager with system follow

## Types

```ts
type SystemTheme = "light" | "dark"
type Theme = SystemTheme | "system"

type ThemeManager = {
  get: () => Theme                    // raw user choice
  resolved: () => SystemTheme          // currently active
  set: (t: Theme) => void
  subscribe: (fn: (resolved: SystemTheme, raw: Theme) => void) => () => void
}
```

## Usage

```ts
import { createThemeManager } from "@basket/theme"
import { createStore } from "@basket/store"
import { emit } from "@basket/ipc"
import { themeChanged } from "../shared/channels"

const settings = createStore("settings", { app: { name: "Notes" } })

const theme = createThemeManager({
  initial: settings.get<"light" | "dark" | "system">("theme") ?? "system",
  onResolved: (t) => emit(themeChanged, { theme: t }),
})

theme.subscribe((resolved, raw) => {
  settings.set("theme", raw)
  // raw is the user's setting ("system"/"light"/"dark");
  // resolved is the currently-active value
})

// Toggle via menu:
onMenu("theme:set-dark",   () => theme.set("dark"))
onMenu("theme:set-light",  () => theme.set("light"))
onMenu("theme:follow-os",  () => theme.set("system"))
```

On the webview, listen for the IPC event and toggle a `data-theme`
attribute on `<body>`. Pair with CSS variables and you get instant
theming.

## Platform detection

- **macOS**: `defaults read -g AppleInterfaceStyle`
- **Linux**: `gsettings get org.gnome.desktop.interface color-scheme`,
  falls back to `$GTK_THEME`
- **Windows**: registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`

If detection fails the manager defaults to `"light"`.

## Depends on

- `butter` (peer) — `on()` for `theme:changed` events (requires the
  `theme` plugin loaded).
