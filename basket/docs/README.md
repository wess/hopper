# Basket Docs

Comprehensive reference for [basket](../README.md) — composable, functional Bun/TypeScript building blocks for desktop apps, built on [butter](https://github.com/wess/butter).

## Start here

| Doc | What it covers |
|---|---|
| [getting-started.md](getting-started.md) | Install, scaffold, run, first-app walkthrough |
| [quickstart.md](quickstart.md) | Build a working notes-style app in ~80 lines |
| [concepts.md](concepts.md) | Host vs webview, channels, events, immutability, no-class rules |
| [overview.md](overview.md) | Architecture, package layers, dependency graph |

## API

| Doc | What it covers |
|---|---|
| [api.md](api.md) | One-screen cross-package API reference |
| [`packages/<name>/AGENTS.md`](../packages) | Canonical per-package reference (open the one you need) |

## How-to guides

| Doc | What it covers |
|---|---|
| [ipc.md](ipc.md) | Typed channels, events, pipelines, validation, errors |
| [data.md](data.md) | SQLite + schemas + migrations + cache + fs + logger |
| [auth.md](auth.md) | Local passwords, OAuth client (PKCE), keychain sessions |
| [ai.md](ai.md) | OpenAI / Anthropic / Ollama providers, streaming, MCP |
| [ui.md](ui.md) | `@basket/ui` components, theming, palette, toast |
| [distribution.md](distribution.md) | Bun → butter compile → bundle → sign → auto-update |
| [testing.md](testing.md) | Unit and end-to-end testing patterns |

## Reference

| Doc | What it covers |
|---|---|
| [cookbook.md](cookbook.md) | Recipes that don't justify a package |
| [troubleshooting.md](troubleshooting.md) | Common errors, platform quirks |
| [comparisons.md](comparisons.md) | Basket vs electron, tauri, atlas |
| [faq.md](faq.md) | Frequently asked questions |
| [contributing.md](contributing.md) | How to contribute |

## Suggested reading order

**New to basket?** [getting-started](getting-started.md) → [concepts](concepts.md) → [quickstart](quickstart.md) → pick a how-to.

**Building a feature?** Open the package's [AGENTS.md](../packages) → consult [api.md](api.md) for cross-package patterns → [cookbook](cookbook.md) for non-obvious recipes.

**Comparing to alternatives?** [comparisons](comparisons.md) → [overview](overview.md).

**Stuck?** [troubleshooting](troubleshooting.md) → [faq](faq.md).
