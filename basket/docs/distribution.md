# Distribution Guide

How to take a working basket project from `bun run dev` to a signed,
distributable binary, and update users in the field.

## The pipeline

```
  src/                bun run dev      → develop
   ↓                  bun run build    → single binary
  butter compile      bun run bundle   → .app / .msi / .deb
   ↓                  butter sign      → notarised / signed
  bundle artifact     publish manifest → users auto-update
```

`basket dev`/`build`/`bundle` are thin wrappers that delegate to
`butter dev`/`compile`/`bundle` — the binary build pipeline lives in
butter.

## Build a single binary

```bash
bun run build
```

This calls `bunx butter compile`. Outputs to `dist/<name>` (the binary
itself). On macOS it's a Mach-O executable; on Linux ELF; on Windows
.exe.

The binary includes:

- Your Bun-compiled host (`src/host`)
- Your bundled webview assets (`src/app`)
- Butter's native shim (small Swift / GTK / WebView2 wrapper)

Typical size: ~3-8 MB depending on assets. Compare to Electron's
~150 MB baseline.

## Bundle for the OS

```bash
bun run bundle
```

Calls `bunx butter bundle`. Per platform:

| Platform | Produces |
|---|---|
| macOS  | `dist/<name>.app` |
| Linux  | `dist/<name>.deb` and `<name>.AppImage` |
| Windows | `dist/<name>.msi` |

The bundle reads from `butter.yaml#bundle`:

```yaml
bundle:
  identifier: io.wess.notes
  category: public.app-category.productivity
  urlSchemes:
    - notes
    - x-notes
```

`identifier` is critical — it's the macOS bundle id, the Linux desktop
file `Name`, the Windows AppUserModelID. Pick one and never change it
(it's the keychain `service` key for `@basket/secrets` users).

`urlSchemes` registers custom URL schemes for `@basket/protocol` deep
links. The bundler writes them into `Info.plist` (macOS), the `.desktop`
file (Linux), or the registry (Windows).

## Signing

### macOS

Two-step: codesign + notarize.

```bash
# Codesign (uses your Developer ID from Keychain)
codesign --deep --force --options runtime --sign "Developer ID Application: Your Name (TEAMID)" dist/Notes.app

# Notarize
xcrun notarytool submit dist/Notes.app --keychain-profile "AC_PROFILE" --wait

# Staple
xcrun stapler staple dist/Notes.app
```

`AC_PROFILE` is a keychain item you set up once with `xcrun notarytool store-credentials`. See [Apple's notes](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution).

Butter exposes a `butter sign` command that wraps this — see [butter's docs](https://github.com/wess/butter).

### Windows

Authenticode signing requires a code-signing certificate from a CA:

```bash
signtool sign /tr http://timestamp.digicert.com /td sha256 /fd sha256 /a dist/Notes.msi
```

Without a cert, Windows SmartScreen will warn users on first launch. The
warning eventually clears with installation reputation, but signing is
strongly recommended.

### Linux

`.deb` packages support GPG signing:

```bash
dpkg-sig --sign builder dist/notes_1.0.0_amd64.deb
```

AppImage doesn't need signing but pairs with a checksum file:

```bash
sha256sum dist/Notes-1.0.0.AppImage > dist/Notes-1.0.0.AppImage.sha256
```

## Distribution channels

Pick one or more:

| Channel | Effort | Reach |
|---|---|---|
| Self-host (GitHub Releases, S3) | Low | You handle update prompts |
| Homebrew Cask (macOS) | Medium | Mac users via `brew install --cask` |
| MAS / Microsoft Store / Snap | High | Friction-free for end users; review process |
| Direct download from your site | Low | Most users |

For an internal tool, GitHub Releases + auto-update is the fastest path.

## Auto-update with `@basket/update`

Host a JSON manifest at a stable URL:

```json
{
  "version": "1.2.3",
  "url": "https://releases.example.com/notes/1.2.3/notes-mac.zip",
  "notes": "- Fixed search\n- Added export",
  "sha256": "ab12cd34…"
}
```

Check on launch:

```ts
import { check, download } from "@basket/update"
import { paths } from "@basket/config"
import { notify } from "@basket/notify"

const current = "1.0.0"  // from package.json or build-time inject

const status = await check({
  url: "https://releases.example.com/notes/latest.json",
  current,
})

if (status.available) {
  await notify({
    title: `Update available: ${status.manifest.version}`,
    body: status.manifest.notes ?? "Click to install.",
  })

  const dest = `${paths({ name: "Notes" }).cache}/update-${status.manifest.version}.zip`
  await download({
    url: status.manifest.url,
    to: dest,
    sha256: status.manifest.sha256,
    onProgress: (got, total) => {
      // send progress event to webview
    },
  })

  // Hand off to OS-specific installer:
  //   macOS: unzip + replace .app + relaunch
  //   Windows: run the .msi
  //   Linux:   open the .deb in package manager
}
```

**Always set `sha256`** in production. Combined with HTTPS, it prevents
tampered builds from being installed.

## Versioning

`package.json#version` is your source of truth. Inject it into the
compiled binary so `check()` has the right `current`:

```ts
// src/host/version.ts
export const VERSION = (await Bun.file("package.json").json()).version as string
```

`butter compile` reads `package.json` at build time; the file isn't
present in the bundle, so do this at module load and let Bun's compiler
inline the result.

A simpler approach: pass via env var:

```bash
BUILD_VERSION=1.2.3 bun run build
```

```ts
export const VERSION = Bun.env.BUILD_VERSION ?? "0.0.0-dev"
```

## Release checklist

Before shipping `1.2.3`:

- [ ] Bump `package.json#version`
- [ ] Update changelog (in your repo, not in basket)
- [ ] `bun test` clean
- [ ] `bun run check` clean
- [ ] `bun run build && bun run bundle` succeeds on each target OS (CI matrix)
- [ ] Smoke-test the bundled app — launch, sign in, exercise the main flows, check `paths(app).logs/app.log`
- [ ] Codesign + notarize / Authenticode sign / sha256 the artifact
- [ ] Upload to release channel
- [ ] Publish manifest (`latest.json`) pointing at the new artifact + sha256
- [ ] Tag the commit

CI does most of this; cross-OS builds need a matrix (GitHub Actions has
macOS/Linux/Windows runners).

## Bundle size hygiene

Common bloat sources:

- **Unused `@basket/*` packages.** Use `basket add` rather than pulling everything.
- **Webview assets.** Image-heavy or big React component libs balloon `src/app`. Run `bun build src/app/main.ts --target browser --minify` to see the output and inspect.
- **MCP SDK.** ~1 MB. Skip `@basket/mcp` if you're not exposing MCP.
- **lucide-react.** Tree-shakes per-icon — `import { Search } from "lucide-react"` is fine; `import * as Icons` is not.

Target: < 10 MB for a fully-featured app. Anything more, audit.

## Anti-patterns

- **Don't ship without `sha256` in the update manifest.** Hash mismatch is your only line of defense against an attacker swapping the binary.
- **Don't change `bundle.identifier` across versions.** It breaks keychain entries, deep links, and app-data continuity for users.
- **Don't auto-install on launch.** Notify, let the user click. Forced restarts mid-edit are the worst sin a desktop app can commit.
- **Don't skip notarization on macOS.** Without it, users hit "<App> can't be opened because Apple cannot check it for malicious software" and have to System-Settings their way past it. Most won't.

## Next

- [butter's docs](https://github.com/wess/butter) — the compile/bundle/sign details
- [`packages/update/AGENTS.md`](../packages/update/AGENTS.md) — update API
- [troubleshooting.md](troubleshooting.md) — platform-specific gotchas
