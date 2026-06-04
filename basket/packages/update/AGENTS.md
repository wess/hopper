# @basket/update

Lightweight auto-update primitives: manifest checks, SHA-256-verified
downloads, version comparison.

## Exports

- `check({ url, current, headers? })` → `UpdateCheck`
- `download({ url, to, sha256?, onProgress? })` → path
- `parseVersion(v)`, `compareVersions(a, b)`, `isNewer(remote, current)`

## Types

```ts
type Manifest = {
  version: string
  url: string
  notes?: string
  sha256?: string
}

type UpdateCheck =
  | { available: false; current: string }
  | { available: true; current: string; manifest: Manifest }
```

## Usage

```ts
import { check, download } from "@basket/update"
import { paths } from "@basket/config"
import { notify } from "@basket/notify"

const status = await check({
  url: "https://releases.example.com/myapp/latest.json",
  current: "1.0.0",
})

if (status.available) {
  await notify({
    title: `Update available: ${status.manifest.version}`,
    body: status.manifest.notes ?? "Click to install.",
  })

  const dest = `${paths({ name: "MyApp" }).cache}/update-${status.manifest.version}.zip`
  await download({
    url: status.manifest.url,
    to: dest,
    sha256: status.manifest.sha256,
    onProgress: (got, total) => console.log(`${got}/${total ?? "?"}`),
  })

  // hand off to platform-specific install (open the .dmg, run the .msi, etc.)
}
```

## Manifest format

Host a JSON file at a stable URL:

```json
{
  "version": "1.2.3",
  "url": "https://releases.example.com/myapp/1.2.3/myapp-mac.zip",
  "notes": "- Fixed search\n- Added export",
  "sha256": "deadbeef..."
}
```

`check()` returns `{ available: false }` if the local `current` version
is ≥ `manifest.version` per `compareVersions`. The comparison is a
simple dot-separated integer compare — no semver pre-release semantics.

## Verifying downloads

If you provide `sha256`, `download()` re-hashes the bytes after fetch
and throws on mismatch. **Always set this** for production updates.
Combine with code signing (`butter sign`) for end-to-end integrity.

## Out of scope

Installation. After download, the app must trigger the OS update flow
(open the DMG, run the installer, replace the binary, restart). That's
platform-specific and lives in your app, not in basket.

## Depends on

Nothing. Uses `fetch`, `crypto.subtle`, `Bun.write`.
