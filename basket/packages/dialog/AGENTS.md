# @basket/dialog

Native file, folder, and message dialogs. Sugar over butter's `dialog`
API with friendlier single/multi variants.

## Exports

- `openFile(opts?)` → `string | undefined` — open one file
- `openFiles(opts?)` → `string[] | undefined` — open multiple files
- `saveFile(opts?)` → `string | undefined` — show save dialog
- `openFolder(opts?)` → `string | undefined` — pick a folder
- `message(opts)` → `MessageDialogResult` — full message dialog
- `alert(msg, title?)` → `void` — info dialog with OK
- `confirm(msg, title?)` → `boolean` — Cancel/OK dialog

All return `undefined` (or `false` for `confirm`) when the user cancels.

## Types (re-exported from butter)

```ts
type FileFilter = { name: string; extensions: string[] }

type OpenDialogOptions = {
  title?: string; prompt?: string
  defaultPath?: string
  filters?: FileFilter[]
}

type SaveDialogOptions = OpenDialogOptions & { defaultName?: string }

type MessageDialogOptions = {
  title?: string
  message: string
  detail?: string
  type?: "info" | "warning" | "error"
  buttons?: string[]
}
```

## Usage

```ts
import { openFile, saveFile, openFolder, alert, confirm } from "@basket/dialog"

const path = await openFile({
  title: "Open Note",
  filters: [{ name: "Markdown", extensions: ["md"] }],
})
if (!path) return

const out = await saveFile({
  defaultName: "export.json",
  filters: [{ name: "JSON", extensions: ["json"] }],
})
if (out) await Bun.write(out, JSON.stringify(data))

const dir = await openFolder({ prompt: "Choose output folder" })
if (dir) console.log(dir)

await alert("Saved!")

if (await confirm("Discard unsaved changes?")) {
  // …
}
```

Works from both **host** and **webview** sides — butter's dialog
auto-detects context.

## Depends on

- `butter/dialog` (peer)
