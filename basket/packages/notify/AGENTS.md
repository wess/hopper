# @basket/notify

OS notifications.

## Exports

- `notify({ title, body, subtitle? })` → `Promise<void>`

## Usage

```ts
import { notify } from "@basket/notify"

await notify({
  title: "Note saved",
  body: "Your draft has been autosaved.",
})

await notify({
  title: "Update available",
  subtitle: "v2.1.0",
  body: "Click to install.",
})
```

## Platform notes

- **macOS**: `osascript` — appears in Notification Center; the user must
  have granted notification permission for the bundling parent
  (Terminal during dev, the bundled app post-release).
- **Linux**: `notify-send` (libnotify). Requires a notification daemon
  (most desktop environments have one).
- **Windows**: PowerShell + `NotifyIcon` balloon tip. Auto-dismisses
  after 5s.

For interactive notifications (actions, replies), use butter's
`notifications` plugin directly — basket's wrapper is for fire-and-forget
toasts.

## Depends on

Nothing. Shells out to platform commands.
