---
title: From source
group: Reference
order: 4
summary: Build it, test it, and how a release is cut.
---

Rust throughout. No Bun, no npm.

```sh
cargo run -p app                 # launch the app (dev binary: hopperdev)
cargo build --workspace          # build everything
cargo test                       # the whole suite
cargo clippy --all-targets       # lint
cargo run -p mcp                 # the stdio MCP server
cargo run -p site                # rebuild this documentation site
```

A source build behaves exactly like the shipped one. Hopper runs no VM of its
own, so it needs no entitlement and no code-signing dance to reach its engine —
Apple's `container` does the virtualizing under its own privileged helpers.

That was not always true: the VM needed a Developer ID to boot, which meant a
`cargo run` build could never exercise the engine. Removing it removed the
problem.

## Conventions

- Functional style; a free function and plain data over a class.
- The gpui-free core (`model`, `store`, `docker`, `apple`, `engine`, `migrate`,
  `host`, `mcp`) never imports gpui. The boundary is `app`.
- File names lowercase, no separators; split by directory, not compound names.
- Pure logic carries the unit coverage — parsers, arg builders, framing,
  reducers. Integration seams are where the real bugs hide, so verify those
  against a live engine.

## This site

Markdown in `site/content/`, a small Rust generator in `crates/site`, output
committed to `docs/`, which is what the Pages workflow publishes. The generated
HTML is committed rather than built in CI so the site stays reviewable in the
diff and a broken generator cannot take it down.

```sh
cargo run -p site
```

The landing page is hand-authored and left alone.

## Cutting a release

Bump the workspace version in the root `Cargo.toml` and push to `main`. That is
the whole trigger. CI runs the gate (tests, `clippy -D warnings`, a release
build), produces a signed and notarized `Hopper.dmg`, publishes the GitHub
release, and updates the Homebrew cask.

The release job also asserts the app carries **no** virtualization entitlement.
Nothing virtualizes any more, and carrying it would quietly widen the sandbox.
