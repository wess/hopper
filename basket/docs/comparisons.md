# Basket vs Alternatives

Honest, technical comparison. No marketing.

## TL;DR

| | Bundle size | Lang | Webview | Native deps |
|---|---|---|---|---|
| **basket + butter** | ~3-8 MB | TS / Bun | System | Tiny shim |
| **Electron** | ~150 MB | TS / Node | Chromium (bundled) | Chromium + Node |
| **Tauri** | ~10-20 MB | TS + Rust | System (Wry) | Rust toolchain |
| **NW.js** | ~120 MB | TS / Node | Chromium (bundled) | Chromium + Node |
| **Wails** | ~10-20 MB | TS + Go | System | Go toolchain |
| **Atlas** | N/A | TS / Bun | None (server-side) | — |

Picking between these is mostly about **toolchain comfort** (Rust vs Go vs pure TS) and **bundle-size tolerance** (a 150 MB download for a 200-line app may be fine for B2B; consumer apps care).

## Basket vs Electron

| Dimension | Basket | Electron |
|---|---|---|
| Bundle size | 3-8 MB | 100-200 MB |
| Renderer | Native webview | Bundled Chromium |
| Backend lang | TS on Bun | TS / JS on Node |
| Auto-update | `@basket/update` (manifest + sha) | Squirrel / electron-updater |
| Native menus, tray, dialogs | Butter primitives | Built-in |
| Security model | Webview isolated; IPC explicit | webPreferences, contextBridge |
| Ecosystem | Small, growing | Huge |
| Memory at idle | ~30-80 MB | ~200-400 MB |

**Pick basket if**: bundle size matters, you want TS-only on Bun, you don't need Chromium-specific features (DevTools protocol, web testing libraries that assume Chrome).

**Pick Electron if**: you need broad library compatibility, your team is deep in Node, you need consistent rendering across platforms (basket inherits each OS's webview behavior).

## Basket vs Tauri

| Dimension | Basket | Tauri |
|---|---|---|
| Backend lang | TS on Bun | Rust (TS via JS interop) |
| Bundle size | ~3-8 MB | ~10-20 MB |
| Renderer | Native webview | Native webview (Wry) |
| Backend deps | `bun:sqlite`, Bun stdlib | Rust crates ecosystem |
| Build complexity | `bun run build` | Rust toolchain required on each dev/CI machine |
| Type sharing across IPC boundary | Trivial (shared TS file) | Possible via codegen / specta |
| Performance ceiling | High (Bun is fast) | Highest (native Rust) |

**Pick basket if**: you don't want Rust in your stack, you want TS everywhere, your performance needs are modest (most desktop apps).

**Pick Tauri if**: you have Rust experts, you need CPU-bound work on the backend (image processing, ML inference outside of an external LLM, large file parsing), you want the most established small-bundle ecosystem.

## Basket vs Atlas

Sister projects. Same author, same conventions, different target.

| Dimension | Basket | Atlas |
|---|---|---|
| Target | Desktop apps (host + webview) | Web apps (server + browser) |
| Runtime | Bun + butter | Bun only |
| Networking | IPC over butter | HTTP over Bun.serve |
| Persistence | `bun:sqlite` | Postgres + SQLite |
| Auth | OAuth *client*, local sessions, keychain | OAuth *server*, password+JWT+session, server-side cookies |
| UI | React desktop primitives (titlebar, palette, …) | React + Mantine, forms, tables |
| MCP | Expose app to AI clients | Expose server to AI clients (debugging) |

The two share core ideas: composable packages, no classes, Bun-only,
biome-only, AGENTS.md per package. You can use both in one product —
a basket desktop client paired with an atlas server is the canonical pairing.

## Basket vs PWA

A Progressive Web App in a browser:

- Doesn't need install
- Limited native access (no menubar, no tray, weak fs)
- Can't ship a binary
- Cross-OS for free

**Pick a PWA if**: native features aren't critical, distribution is a website, your user base accepts logging in to a URL.

**Pick basket if**: you need menubar/tray/dialog/keychain/global shortcuts, or you want a binary in the App Store / Microsoft Store.

## Why "atlas, but for desktop"?

Atlas formed a clean opinion about how a Bun-native composable framework should look — small packages, AGENTS.md per package, functional, vendored not published. Basket applies that same opinion to butter, the way Phoenix applied Plug's opinion to web frameworks for Elixir.

Same author, same conventions, same AI-friendly per-package docs. If you've used atlas, basket should feel familiar. The package boundaries differ (no `@basket/server` because the IPC layer replaces it; no `@atlas/window` because the web has no windows) but the *spirit* is the same.

## When NOT to pick basket

- **You need to ship today and your team has zero Bun experience.** Bun has rough edges relative to Node; pick Electron.
- **You target ChromeOS, iOS, Android, or web in addition to desktop.** Basket is desktop-only; use a cross-platform framework (Tauri 2, Flutter, React Native).
- **You need plugin-based extensibility for end-user-written plugins.** Basket has no dynamic plugin loader; user-written extensions would require shipping a Bun sandbox. Possible, but not built-in.
- **Your app is fundamentally a browser experience that happens to run as a desktop window.** A PWA wrapped in WKWebView / Edge is simpler.

## Next

- [overview.md](overview.md) — basket's architecture in depth
- [getting-started.md](getting-started.md) — try it
