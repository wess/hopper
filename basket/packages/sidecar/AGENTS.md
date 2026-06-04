# @basket/sidecar

Spawn external executables shipped alongside the app — ffmpeg, yt-dlp,
a Go daemon, anything that's easier to use as a subprocess than to
re-implement in TypeScript.

Reimplements butter's `sidecar` plugin in the host process so no IPC
roundtrip is involved. Resolution still keys off the env vars butter
sets (`BUTTER_SIDECARS` in dev, `BUTTER_SIDECARS_DIR` in compiled
bundles) — sidecars are declared once in `butter.yaml`:

```yaml
bundle:
  sidecars:
    - bin/ffmpeg
    - bin/yt-dlp
```

## Exports

- `spawn(name, options?)` → `Sidecar` — start the binary.
- `listSidecars()` → `readonly string[]` — names declared in `butter.yaml`.

```ts
type SpawnOptions = {
  readonly args?: readonly string[]
  readonly cwd?: string
  readonly env?: Readonly<Record<string, string>>
}

type Sidecar = {
  readonly name: string
  readonly pid: number
  readonly write: (text: string) => void
  readonly kill: (signal?: NodeJS.Signals | number) => void
  readonly onStdout: (h: (chunk: string) => void) => () => void
  readonly onStderr: (h: (chunk: string) => void) => () => void
  readonly exited: Promise<number | null>
}
```

`onStdout` / `onStderr` return unsubscribe functions. Streams pump for
the lifetime of the process; chunks are UTF-8-decoded as they arrive.

## Usage

```ts
import { spawn } from "@basket/sidecar"

const ff = spawn("ffmpeg", { args: ["-i", input, "-vf", "scale=720:-1", output] })
ff.onStderr((chunk) => console.error(`[ffmpeg] ${chunk}`))

const code = await ff.exited
if (code !== 0) throw new Error(`ffmpeg exited ${code}`)
```

```ts
// Stream the agent's stdout into a logger
import { spawn } from "@basket/sidecar"
import { createLogger } from "@basket/logger"

const log = createLogger({ name: "agent" })
const agent = spawn("agent", { args: ["--port", "9000"] })
agent.onStdout((c) => log.info(c.trimEnd()))
agent.onStderr((c) => log.warn(c.trimEnd()))
```

## Naming

Sidecars are addressed by basename:

| Declared | Looked up as |
|---|---|
| `bin/ffmpeg` | `"ffmpeg"` |
| `bin/yt-dlp` | `"yt-dlp"` |
| `tools/agent.exe` | `"agent"` (the `.exe` is stripped) |

If the same basename appears twice in `bundle.sidecars`, the last one
wins.

## Depends on

- None at runtime. Uses `Bun.spawn` directly.
