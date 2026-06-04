# Getting Started

## Requirements

- [Bun](https://bun.com) ≥ 1.2
- macOS, Linux, or Windows
- ~30 MB disk space for `node_modules` + butter's native shim
- Git, if you plan to track your project

Run `bun --version` to confirm Bun is installed. If you don't have it:

```bash
curl -fsSL https://bun.sh/install | bash
```

## Two ways to start

### A. Scaffold a new project (recommended)

```bash
bunx --bun @basket/cli init myapp --template minimal
cd myapp
```

This drops a working butter project pre-wired with five `@basket/*` workspace deps (`config`, `store`, `window`, `ipc`, `menu`).

Available templates:

| Template | Best for |
|---|---|
| `minimal` | Single-window app — productivity tool, viewer, utility |
| `menubar` | Tray-only / popover app — small focused tool |

```bash
bunx --bun @basket/cli init myapp --template menubar
```

### B. Add basket to an existing butter project

```bash
# From the project root
curl -sL https://github.com/wess/basket/archive/refs/heads/main.zip -o /tmp/basket.zip
unzip -q /tmp/basket.zip -d /tmp/basket-expand
mv /tmp/basket-expand/basket-main ./basket
rm -rf /tmp/basket.zip /tmp/basket-expand
echo "basket/" >> .gitignore
```

Then in your `package.json`:

```json
{
  "workspaces": ["basket/packages/*"],
  "dependencies": {
    "butter": "npm:butterframework@latest",
    "@basket/config": "workspace:*",
    "@basket/store": "workspace:*",
    "@basket/window": "workspace:*",
    "@basket/ipc": "workspace:*"
  }
}
```

`bun install` resolves the workspace and `@basket/*` aliases through basket's `tsconfig.json#paths`.

## Vendor philosophy

Basket is **not on npm**. You clone or vendor it as a workspace next to your code, then reference packages with `workspace:*`. Why:

- **Atomic edits.** Change basket source and your app picks it up immediately. No `npm publish`, no version skew.
- **Source-readable.** Cmd-click any `@basket/*` import and you land in the source. AI sessions don't have to guess at API shapes.
- **No semver tax.** With ~25 small packages, semver discipline costs more than the safety gives.

Trade-off: you don't get a "lock to version 1.0.3" guarantee. If that matters, fork basket and tag the fork.

## Project layout

After `basket init myapp --template minimal`:

```
myapp/
├── butter.yaml              # window + build + bundle config
├── package.json             # workspaces + deps + scripts
├── src/
│   ├── shared/
│   │   └── channels.ts      # IPC channel definitions (host + webview both import)
│   ├── host/
│   │   ├── index.ts         # main host: config, store, ipc handlers, menu
│   │   └── menu.ts          # @basket/menu definition
│   ├── app/
│   │   ├── index.html       # webview entry
│   │   ├── main.ts          # webview logic (uses @basket/ipc/client)
│   │   └── styles.css
│   └── env.d.ts             # global butter type for the webview
└── .gitignore
```

## Run it

```bash
bun install
bun run dev
```

This invokes `butter dev` under the hood:

1. Type-checks and bundles `src/host/index.ts` for Bun
2. Type-checks and bundles `src/app/*` for the webview
3. Launches the native shim (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux)
4. Hot-reloads on file changes

You should see a native window. ⌘T toggles the theme; ⌘Q quits. The greeting text comes from a `handle(greet, ...)` call in `src/host/index.ts` that the webview invokes on load.

## Add more packages

```bash
basket add db migrate request auth
bun install
```

This appends the four packages to `package.json#dependencies` as `workspace:*`. Edit `src/host/index.ts` to use them — see [data.md](data.md), [auth.md](auth.md), or any `packages/<name>/AGENTS.md` for usage.

## Ship a binary

```bash
bun run build       # → basket build → butter compile
bun run bundle      # → basket bundle → butter bundle (.app, .msi, .deb)
```

See [distribution.md](distribution.md) for signing, notarisation, and auto-update.

## What's next

- **Understand the model**: [concepts.md](concepts.md) — host vs webview, channels, immutability.
- **Build something real**: [quickstart.md](quickstart.md) — full notes-style app in one screen.
- **Pick the right tool**: [api.md](api.md) — cross-package cheat sheet.
- **Pattern catalogue**: [cookbook.md](cookbook.md) — recipes for non-obvious situations.
