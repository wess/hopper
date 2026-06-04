# @basket/cli

The `basket` CLI: scaffolding, dev, build, docs, doctor.

## Bin

`basket` (registered via `package.json#bin`). Run it with `bunx basket <cmd>` once the package is in your workspace.

## Commands

| Command | Description |
|---------|-------------|
| `basket init <name> [--template <t>]` | Scaffold a new project from a template (`minimal`, `menubar`) |
| `basket dev` | Spawn `bunx butter dev` in the current directory |
| `basket build` | Spawn `bunx butter compile` (single-binary build) |
| `basket bundle` | Spawn `bunx butter bundle` (OS-native packaging) |
| `basket add <pkg>…` | Append `@basket/*` workspace deps to the current `package.json` |
| `basket docs [target]` | Print a package's `AGENTS.md` or a top-level `docs/*.md` |
| `basket doctor` | Verify bun + butter are available |

## Templates resolved

Templates live in `templates/<name>/`. `init` walks the directory, drops
the `.tmpl` extension (if present) on each file, substitutes
`{{name}}` with the project name in both file paths and file contents,
and writes the result to `<cwd>/<name>/`.

Binary files (`.png`, `.icns`, etc.) are copied byte-for-byte without
substitution.

## Programmatic use

```ts
import { scaffold } from "@basket/cli"

await scaffold({ name: "myapp", template: "minimal" })
```

## Exports

- `cli(name, commands)` — top-level CLI runner
- `command(name, opts)`, `flag(short, opts)`, `parseArgs(argv, defs)` — building blocks
- `initCommand`, `devCommand`, `buildCommand`, `bundleCommand`, `addCommand`, `docsCommand`, `doctorCommand` — pre-wired commands
- `scaffold(opts)` — programmatic project scaffolding

## Depends on

Nothing. Zero dependencies. Spawns `bunx butter` for `dev`/`build`/`bundle`/`doctor`.
