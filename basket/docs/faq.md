# FAQ

## Why isn't basket on npm?

Same answer as atlas: vendoring is faster to iterate on, source is the
documentation, AI sessions don't have to guess. The 25-package surface
would also be a lot of semver discipline. If you want a published pin,
fork basket and tag the fork.

## Why Bun?

- Bun has SQLite, password hashing, fast HTTP, shell scripting, atomic
  file writes, and a test runner built in. Each of those is a third-party
  npm dep on Node. Basket inherits all of them for free.
- `bun:sqlite` is faster than `better-sqlite3` and ships with the
  runtime. No native module rebuild dance.
- The TS-as-source experience (no transpile step) makes "vendor and go"
  realistic. With Node + tsc, you'd be compiling basket on every
  install.

## Why butter, not Electron / Tauri / Wails?

We wanted:
- TS on both sides (no Rust, no Go)
- System webview (no Chromium bloat)
- Bun's stdlib for the backend

Butter is the only existing project that combines those. If butter
didn't exist, basket would be glued to a different shim. The choice is
about backend language, not religion.

## Can I use basket without butter?

In theory yes — basket's data, request, secrets, auth, ai, mcp, fs,
cache, logger packages have no butter dep. You could use them on a Bun
HTTP server. But the IPC, window, menu, tray, dialog, notify, shortcut,
update, protocol, lifecycle, theme packages all wrap butter.

If you don't need a desktop UI, you probably want [atlas](https://github.com/wess/atlas).

## Can I use React / Vue / Svelte for the webview?

Yes. Butter ships templates for vanilla, React, Svelte, and Vue. Basket's
`@basket/ui` is React-only, so if you pick Svelte/Vue you'll write your
own UI primitives — but everything else (IPC, db, auth, …) works
identically.

## Why no class components / OOP?

Functional code composes better, doesn't hide state behind `this`, and
is trivially testable. Classes encourage hierarchies that AI-generated
code tends to over-extend. Pure data + functions is also the easiest
shape for AI to reason about, which matches basket's AI-friendly
positioning.

The one exception: `Error` must be a class to throw with a stack trace.
Basket tags Error instances with `Object.assign` rather than
subclassing. The user-facing API never returns Error instances directly;
factories like `notFound()` return them.

## How big is the bundle?

Bun-compiled basket app, with most packages used, sits around 3-8 MB
depending on assets. Butter's native shim adds another ~1 MB. Compare
to Electron's 150 MB baseline.

## Does basket support hot reload?

`butter dev` watches files and reloads the webview. The host bundle
restarts on change. Native state (databases, keychain) survives reload.

## How do I share types between host and webview?

Put them in `src/shared/`. Both `tsconfig` views include it. The
canonical example is `src/shared/channels.ts` for IPC channel
definitions.

## Can I use a different database?

`@basket/db` is `bun:sqlite`. For Postgres, use atlas's `@atlas/db` —
they have compatible APIs. For NoSQL, none of basket wraps it; use
`@basket/request` to talk to MongoDB / Firestore / etc.

## Can two basket processes share the same SQLite db?

Yes, with WAL mode (which `connect()` enables). The reader-writer pattern
is safe. Multiple writers from different processes work but can hit
"database is locked" — wrap writes in transactions, bump the busy
timeout if needed.

The MCP pattern (run an MCP server next to the app, sharing the db) is
this exact case.

## How does basket handle deep links / custom URL schemes?

Declare schemes in `butter.yaml#bundle.urlSchemes`, register
`@basket/protocol` handlers on the host. See
[`packages/protocol/AGENTS.md`](../packages/protocol/AGENTS.md). The
butter shim emits `app:openurl` when a deep link arrives; basket parses
it into `{ scheme, host, path, params }` and routes to your handler.

## How do I bundle and sign for the App Store?

Butter's `compile` + `bundle` produce an unsigned `.app`. For App Store
distribution you need:

1. An Apple Developer account
2. A Distribution provisioning profile
3. `codesign` + `productbuild` + `xcrun altool` flow

See [distribution.md](distribution.md#macos). The basket side stops at
bundle; signing is OS plumbing that butter wraps via `butter sign`.

## Why no `@basket/server`?

A desktop app's "server" is its host process — and the host talks to
the webview via IPC, not HTTP. If your desktop app *also* runs an HTTP
server (for an integrated dashboard, webhook receiver, MCP transport),
use `Bun.serve` directly. It's already in Bun; basket doesn't need to
wrap it.

## Can the webview access the filesystem directly?

No, by design. The webview is sandboxed in the OS's native renderer.
Fs access goes through `@basket/ipc` to the host. This is the security
boundary — your webview HTML can't read `~/.ssh/id_rsa` even if a script
in your bundle (or an injected script) tries.

## Is there a router for the webview?

Use whatever you want — React Router, TanStack Router, vanilla
`window.location`. Basket doesn't ship one because the webview is your
UI library's domain, not the framework's. The webview is a normal
single-page-app context with `history.pushState`, etc.

## How do I open a second window?

`openWindow({ url, defaults: {...}, store, storeKey })` from
`@basket/window`. Each window has its own webview process. They
communicate by both being subscribed to the same `Event` channels.

## What's the difference between `@basket/store` and `@basket/cache`?

| | Store | Cache |
|---|---|---|
| Purpose | User-visible state | Performance acceleration |
| TTL? | No | Yes |
| Location | `paths.config` | `paths.cache` |
| What goes here | Theme preference, last selected note, sidebar width | API responses, computed thumbnails, parsed manifests |
| If I delete the file? | Bad — user loses their settings | Fine — re-fetch on next use |

Rule of thumb: if losing this data should require a deliberate "clear
cache" action, it's a `Store`. If it's safe to delete on a whim, it's a
`Cache`.

## Why no `@basket/email`?

Most desktop apps don't send email; users send email via their own
email client. If yours genuinely needs to (transactional notifications),
call an API like Resend / Postmark from `@basket/request`.

## Is basket production-ready?

Patterns are stable, tests cover the load-bearing paths, biome is
clean, types are strict. Some packages (especially `@basket/update`'s
post-download install step, `@basket/protocol`'s deep-link reception on
some platforms) depend on butter shipping the corresponding native
hook. Read the per-package "Caveats" section in AGENTS.md before
relying on something in production.

For internal tools and side projects: yes.
For a paid product with SLAs: write tests for your specific flows, and
expect to contribute upstream when you find rough edges.

## How do I contribute?

See [contributing.md](contributing.md).

## License?

Apache 2.0. See [LICENSE](../LICENSE).

## Why "basket"?

Atlas carries the world; basket holds your stuff. Composable, you pick
what goes in. Bun-shaped. Also rhymes with "task it" if you squint.

## Next

- [troubleshooting.md](troubleshooting.md) — when something's broken
- [comparisons.md](comparisons.md) — basket vs Electron / Tauri / atlas
- [contributing.md](contributing.md) — help build it out
