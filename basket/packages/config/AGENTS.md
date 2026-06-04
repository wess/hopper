# @basket/config

Typed environment variables, lazy config resolution, and platform-correct app paths for desktop apps.

## Exports

- `env(name, opts?)` → `EnvRef<T>` — reference to an env var, resolved lazily
  - `opts.parse`: `(string) => T` — transform the raw string
  - `opts.default`: `string` — fallback if the var is missing
- `defineConfig(schema)` → `Readonly<ResolvedSchema>` — recursively resolves env refs into a frozen config object
- `paths(app)` → `AppPaths` — platform-correct user paths for the app
  - macOS: `~/Library/{Application Support,Preferences,Caches,Logs}/<name>`
  - Linux: XDG-style — `$XDG_DATA_HOME`, `$XDG_CONFIG_HOME`, `$XDG_CACHE_HOME`, `$XDG_STATE_HOME/<name>/logs`
  - Windows: `%APPDATA%\<name>`, `%LOCALAPPDATA%\<name>\{Cache,Logs}`

## Types

```ts
type EnvRef<T> = { read: () => T }

type AppId = { name: string; id?: string }

type AppPaths = {
  data: string    // user data — db files, app state
  config: string  // user config — settings.json
  cache: string   // disposable — caches, thumbnails
  logs: string    // log files
}
```

## Usage

```ts
import { defineConfig, env, paths } from "@basket/config"

const config = defineConfig({
  app: {
    name: "Notes",
    id: "io.wess.notes",
    version: "1.0.0",
  },
  db: {
    file: env("DB_FILE", { default: "notes.db" }),
  },
  features: {
    cloudSync: env("CLOUD_SYNC", { parse: (v) => v === "true", default: "false" }),
  },
})

// config.app.name → "Notes"
// config.db.file → string
// config.features.cloudSync → boolean

const p = paths(config.app)
// p.data    → ~/Library/Application Support/Notes (macOS)
// p.config  → ~/Library/Preferences/Notes
// p.cache   → ~/Library/Caches/Notes
// p.logs    → ~/Library/Logs/Notes
```

`paths()` returns the canonical directory for each role on the current
platform. The directories are not created — pass them to `mkdir -p` style
helpers (`Bun.write` to a path inside `data` will auto-create parents).

Bun loads `.env` files automatically. Don't use `dotenv`.

## Depends on

Nothing. Zero dependencies.
