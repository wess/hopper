# Contributing

Basket is a small, opinionated project. PRs welcome, but please read this first.

## Before you start

Open an issue describing what you want to change, especially for:

- New packages
- New top-level commands in `@basket/cli`
- Significant API changes to an existing package
- Anything that adds an external dependency

Small fixes (docs typos, obvious bugs, missing test cases) — just send
the PR.

## Setup

```bash
git clone https://github.com/wess/basket
cd basket
bun install
bun test
bun run check
bunx tsc --noEmit
```

All four should pass before you change anything.

## Conventions

The non-negotiable rules from [SOUL.md](../SOUL.md):

- **Filenames**: lowercase only, no `-` / `_` / spaces. Hierarchy via subdirectories.
- **No classes.** Tagged objects + factory functions. (Error is the
  one allowed exception, and even then we tag instances rather than
  subclassing.)
- **Immutable inputs.** Transforms return new objects.
- **Bun-only runtime.** Reach for `Bun.*` before `node:*`.
- **Butter is the desktop runtime.** Wrap its primitives — don't reinvent.
- **Biome only.** No Prettier, no ESLint.
- **Tests in `packages/<name>/test/`.**

The hard "do nots":

- Don't add classes.
- Don't mutate input data.
- Don't reach for external packages when a `Bun.*` or Web API exists.
- Don't use `as any` to paper over types — fix at the boundary.
- Don't add backward-compat shims, deprecation comments, or `// removed` markers. Basket isn't on npm; delete cleanly.
- Don't add JSDoc that restates what the code does. Comment the *why* only when non-obvious.
- Don't mention Claude / Anthropic / any AI tool in commit messages, PR descriptions, or generated code.

## Adding a new package

```
packages/<name>/
├── package.json
├── tsconfig.json
├── AGENTS.md           ← canonical reference (≤ 200 lines)
├── index.ts            ← public exports
├── <feature>.ts        ← implementation files (keep small + focused)
└── test/
    └── <feature>.test.ts
```

Then:

1. Add the path alias to root `tsconfig.json#paths`
2. Add the package name to `packages/cli/add/index.ts#KNOWN_PACKAGES` and `packages/cli/docs/index.ts#KNOWN_PACKAGES`
3. Add an entry to `llms.txt`, `README.md` (package table), and `docs/api.md`
4. If it has runtime deps, list them in the package's `package.json` and document them in AGENTS.md under "Depends on"

### Package design checklist

- [ ] **Functional only.** No classes. Closures + tagged objects.
- [ ] **Inputs frozen.** `Object.freeze` returned values when reasonable.
- [ ] **Types exported.** Every type a caller might use is named and exported.
- [ ] **Zero deps if possible.** If you must add a dep, justify in AGENTS.md.
- [ ] **AGENTS.md ≤ 200 lines.** Public API, types, usage, depends-on, no fluff.
- [ ] **Tests.** Cover the load-bearing paths. Tests must run in `bun test` under 200 ms each.
- [ ] **`tsc --noEmit` clean.**
- [ ] **`bun run check` clean.**

## Writing AGENTS.md

The model: a single screen of reference that an AI session (or new
developer) can read once and start using the package correctly.

Structure:

```
# @basket/<name>

One-paragraph description of what it does.

## Exports

Bulleted list with signatures.

## Types

Type definitions in a fenced block. Show the shape; don't repeat type
inference details.

## Usage

One or two working examples. Show the canonical happy path.

## Caveats / Why / Notes

The non-obvious. Why we chose this design. Things that will surprise.

## Depends on

What it imports. Empty = "nothing".
```

Keep prose terse. Code blocks should compile (paste them into a scratch
file to verify).

## Commit messages

Imperative present tense. Subject line ≤ 72 chars. Body explains *why*:

```
add ensurePaths helper to @basket/config

Apps need to mkdir -p the four paths before any IO. Doing it
manually in every app's main.ts is boilerplate. ensurePaths runs
the four mkdirs in parallel and returns the resolved paths.
```

Don't reference issue numbers in subjects (they bind to one Git host).
Reference them in the body if needed.

## Pull requests

- Branch from `main`
- One concern per PR
- Update AGENTS.md / docs / llms.txt for any API change
- Pass `bun test`, `bun run check`, `bunx tsc --noEmit`
- Include a test that fails before your fix and passes after

PR description template:

```
## What
One sentence on the change.

## Why
The motivation. Reference issue if relevant.

## How
Anything non-obvious about the implementation.

## Tests
`bun test packages/<name>` — N pass, 0 fail.
```

## Style

Biome handles formatting. Just don't fight it:

```bash
bun run tidy
```

A few patterns we lean into that biome won't catch:

- Prefer `type` over `interface` (consistent with atlas)
- Prefer named exports over default
- Re-export from `index.ts` only — don't re-export from feature files
- Group related functions in one file rather than scattering one-per-file
- File names match the dominant concept inside (`scope.ts` exports `createScope`)

## Testing

Run the suite often. It's fast (< 300 ms across all packages):

```bash
bun test                                # all
bun test packages/db                    # one package
bun test packages/db/test/db.test.ts    # one file
bun test --coverage                     # coverage
```

See [testing.md](testing.md) for patterns. Briefly: use `:memory:`
SQLite, `memoryStore()`, and real `Bun.serve` for HTTP. Avoid mocks
when an in-memory implementation works.

## Documentation

When you change an exported API:

- Update `packages/<name>/AGENTS.md`
- Update `docs/api.md` if the signature appears there
- Update the relevant `docs/*.md` guide if the change is user-visible

The CI guideline (informal): if you can't update the doc, the API isn't
ready.

## What we won't merge

- New packages that duplicate existing ones with cosmetic differences
- Class-based rewrites of functional code
- Adding ESLint / Prettier configuration
- "Util" packages that bundle multiple unrelated helpers
- Code that imports `node:*` when a `Bun.*` API exists
- PRs that touch many packages "while we're here" — split them
- Docs / AGENTS.md changes that add fluff without information

## What we love

- Failing tests for known bugs
- AGENTS.md improvements with concrete examples
- Tightening types (replacing `unknown` with a precise type)
- New test coverage for existing code
- Native-platform fixes (Windows keychain, Linux d-bus, etc.) with clear motivation
- New templates (cookbook, multi-window, editor, etc.) with corresponding examples

## Code of conduct

Be kind, be terse, ship code. Disagreements get resolved in PRs with
data and examples, not in long Slack-style threads.

## Maintainers

The project owner ([wess](https://github.com/wess)) handles all git
operations on `main` and tags. PRs go through `develop`-style review
unless trivial.

## Questions?

Open an issue with the `question` label, or check [faq.md](faq.md) first.
