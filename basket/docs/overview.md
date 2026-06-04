# Overview

Basket is a composable, functional Bun/TypeScript framework for desktop
apps, built on top of [butter](https://github.com/wess/butter). Its
philosophy mirrors atlas's: zero classes, minimal dependencies, tiny
focused packages, AI-friendly, vendored not published.

## Layers

```
┌─────────────────────────────────────────┐
│  your app (host + webview)              │
├─────────────────────────────────────────┤
│  @basket/* packages                     │
│  config | store | ipc | window | menu   │
│  tray | db | cli                        │
├─────────────────────────────────────────┤
│  butter (runtime)                       │
│  on/send/setMenu/createWindow/plugins   │
├─────────────────────────────────────────┤
│  Bun + native shim (WKWebView/WebKitGTK │
│  /WebView2)                             │
└─────────────────────────────────────────┘
```

Each `@basket/*` package wraps a thin slice of butter (or `bun:sqlite`)
and exposes a typed, immutable, functional API. Packages compose by
imports — there is no shared registry, no plugin loader, no DI.

## Host vs webview

Butter splits a desktop app into two TypeScript bundles:

- **Host** (`src/host/index.ts`): runs in Bun. Has the database, file
  system, native primitives, and IPC handlers.
- **Webview** (`src/app/main.ts`): runs in the OS native webview. Has
  DOM and the `butter` global for IPC.

Basket leans into the split:

- `@basket/config`, `@basket/store`, `@basket/db`, `@basket/window`,
  `@basket/menu`, `@basket/tray` are **host-only**.
- `@basket/ipc` exposes a host module (`@basket/ipc`) and a webview
  module (`@basket/ipc/client`). Both consume the same `Channel<I, O>`
  definitions, defined once in a shared file.

Sharing channel definitions across the boundary is the single most
important pattern: it gives you tsserver-checked end-to-end IPC.

## Why no classes

- Tagged objects + closures + factory functions are simpler to compose
  and trivially serialisable.
- Inheritance creates implicit coupling that AI-generated code tends to
  amplify. Functional code can be extended by composition only — there
  is no parent class to change behavior from a distance.
- Easier testing. No mocks, no `instanceof` checks, no protected
  methods. Pure data and functions.

## Why vendor instead of npm

- Atomic updates: change basket source and your app picks it up
  immediately. No cache invalidation, no version skew.
- Total readability. Cmd-click any `@basket/*` import and you're in the
  source.
- AI sessions don't need to fetch published packages or guess at API
  shapes — the source is right there.
- Trade-off: no semver discipline. We mitigate by keeping the package
  count small and the API surface tight.

## Dependency graph

Shallow. Each package depends on at most one or two siblings:

```
config  ← store ← window
        ← cli
        ← db (no sibling deps; bun:sqlite only)
        ← ipc (host) → butter
        ← menu → butter
        ← tray → butter
```

`@basket/cli` depends on nothing else; it spawns `bunx butter` for dev/
build/bundle and walks the templates directory directly.

## What basket is not

- Not a UI framework. The webview is yours — vanilla DOM, React,
  Svelte, Vue, whatever. Pick a butter template.
- Not a state library. Use a regular reactive store on the webview side
  if you want one.
- Not a "build once, run everywhere" framework. Basket inherits butter's
  platform support: macOS, Linux, Windows. There is no mobile story.
- Not a replacement for butter. Basket sits on top. If butter doesn't
  expose what you need, we add to butter, not basket.

## Roadmap

The MVP ships 8 packages. The next batch (in atlas-parity order):

- `@basket/dialog` — typed file/folder dialog wrappers
- `@basket/notify` — OS notifications wrapper
- `@basket/shortcut` — global + scoped keyboard shortcuts
- `@basket/migrate` — versioned SQL migrations
- `@basket/fs` — sandboxed file helpers
- `@basket/logger` — file-rotating logger under `paths.logs`
- `@basket/update` — auto-update wrapper
- `@basket/protocol` — custom URL scheme / deep link handlers
- `@basket/ui` — desktop primitives (titlebar, sidebar, command palette)

Templates: `editor`, `dashboard`, `multiwindow`, `note`, `chatapp`,
`mediaplayer`, `tools`. Examples: at least one per template family.
