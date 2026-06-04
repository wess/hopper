# @basket/protocol

Custom URL scheme / deep-link handling. Parse incoming URLs (`myapp://open?id=1`)
and route them to handlers.

## Exports

- `parseDeepLink(url)` → `DeepLink`
- `onProtocol(scheme, handler)` → unsubscribe
- `onAnyProtocol(handler)` → unsubscribe (catches every scheme)

## Types

```ts
type DeepLink = {
  scheme: string                      // "myapp"
  host: string                        // "open"  (from myapp://open/...)
  path: string                        // "/foo"
  params: Readonly<Record<string, string>>
  url: string                         // original
}
```

## Register the scheme

In `butter.yaml`:

```yaml
bundle:
  identifier: io.wess.notes
  urlSchemes:
    - notes
    - x-notes
```

Run `butter bundle`. The bundled app declares the schemes so the OS
routes `notes://…` to your app.

## Handle incoming links

```ts
import { onProtocol } from "@basket/protocol"

onProtocol("notes", async (link) => {
  if (link.host === "open" && link.params.id) {
    selectNote(Number(link.params.id))
  }
})
```

`onProtocol` listens for butter's `app:openurl` event and routes the URL
to the scheme-matching handler. If no scheme handler matches, an
`onAnyProtocol` handler (if registered) receives it.

## Parser without listener

```ts
import { parseDeepLink } from "@basket/protocol"

const link = parseDeepLink("notes://open?id=42&focus=true")
// { scheme: "notes", host: "open", path: "/", params: { id: "42", focus: "true" }, url: "notes://..." }
```

## Caveats

- The `app:openurl` event must be emitted by butter's shim on
  deep-link arrival. Older butter versions may not emit it — verify
  with a `console.log` in your handler.
- On macOS, deep links arriving while the app is **not running** are
  buffered by the OS and delivered after launch. On Linux/Windows the
  app is launched fresh with the URL as a CLI arg; basket does not
  parse `process.argv` automatically — call `parseDeepLink(process.argv[2])`
  yourself at startup if `argv[2]` looks like a URL.

## Depends on

- `butter` (peer) — for the `on()` event listener.
