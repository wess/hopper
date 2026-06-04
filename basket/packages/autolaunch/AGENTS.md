# @basket/autolaunch

Register the app to run automatically at user login. Per-user, no admin
rights required. Reimplements the platform plumbing directly (matches
the `@basket/notify` pattern) so no butter plugin needs to be loaded.

| Platform | Mechanism |
|---|---|
| macOS | `~/Library/LaunchAgents/<appId>.plist` + `launchctl load` |
| Linux | `~/.config/autostart/<appId>.desktop` |
| Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\<appId>` |

## Exports

- `enable(opts)` → `Promise<void>` — install the autostart entry.
- `disable(appId)` → `Promise<void>` — remove the entry. No error if absent.
- `isEnabled(appId)` → `Promise<boolean>` — does the entry exist right now.

```ts
type AutoLaunchOptions = {
  readonly appId: string            // [a-zA-Z0-9._-]+ — usually bundle.identifier
  readonly displayName?: string     // shown in Linux startup-apps UIs
  readonly args?: readonly string[] // forwarded to the binary at launch
  readonly exePath?: string         // defaults to process.execPath
}
```

## Usage

```ts
import { enable, isEnabled, disable } from "@basket/autolaunch"

const APP_ID = "com.example.myapp"

if (!(await isEnabled(APP_ID))) {
  await enable({
    appId: APP_ID,
    displayName: "My App",
    args: ["--background"],
  })
}

// later — let the user toggle it off
await disable(APP_ID)
```

## Gotchas

- On macOS the plist points at `process.execPath`, which during dev is
  Bun itself. Set `exePath` to your compiled binary, or only enable in
  packaged builds.
- On Windows, single quotes in `args` must already be escaped — we
  double-quote each arg and double up embedded quotes.
- The user can disable any of these from the OS UI (Login Items,
  Startup Applications, Task Manager → Startup). Treat `isEnabled` as
  the source of truth; don't cache it.

## Depends on

- None at runtime. Uses `Bun.$` for `launchctl` / `reg`.
