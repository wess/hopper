# AI Guide

`@basket/ai` and `@basket/mcp` add LLM features to desktop apps.

## What's in @basket/ai

A common `Provider` interface across three implementations:

| Provider | Chat | Streaming | Embeddings |
|---|---|---|---|
| `openai`    | ✅ | ✅ | ✅ |
| `anthropic` | ✅ | ✅ | — |
| `ollama`    | ✅ | ✅ | ✅ (local) |

```ts
type Provider = {
  name: string
  chat: (req) => Promise<ChatResponse>
  chatStream: (req) => AsyncIterable<{ delta: string; done: boolean }>
  embed?: (req) => Promise<EmbedResponse>
}
```

Pick one or accept a Provider as a parameter — swap at runtime.

## Quick start

```ts
import { anthropic } from "@basket/ai"

const ai = anthropic({ apiKey: Bun.env.ANTHROPIC_API_KEY ?? "" })

const reply = await ai.chat({
  system: "You are a helpful assistant. Be terse.",
  messages: [{ role: "user", content: "Summarise this note in one sentence." }],
})
console.log(reply.content)
```

`reply.content` is the assistant message; `reply.usage` reports tokens; `reply.raw` is the full provider response if you need it.

## Streaming

```ts
for await (const chunk of ai.chatStream({ messages: [...] })) {
  if (chunk.delta) process.stdout.write(chunk.delta)
  if (chunk.done) break
}
```

To stream a response to the webview, emit each chunk:

```ts
import { defineEvent, emit, handle } from "@basket/ipc"

export const chatChunk = defineEvent<{ id: string; delta: string }>("chat:chunk")
export const chatDone  = defineEvent<{ id: string }>("chat:done")

handle(askChannel, async ({ id, messages }) => {
  for await (const chunk of ai.chatStream({ messages })) {
    if (chunk.delta) emit(chatChunk, { id, delta: chunk.delta })
    if (chunk.done) {
      emit(chatDone, { id })
      break
    }
  }
  return { ok: true }
})
```

Webview side:

```ts
subscribe(chatChunk, ({ id, delta }) => {
  if (id !== currentChatId) return
  appendToCurrentMessage(delta)
})
subscribe(chatDone, () => setStreaming(false))
```

## Provider swap

Make your app provider-agnostic by accepting `Provider` everywhere:

```ts
import { type Provider, anthropic, openai, ollama } from "@basket/ai"

const pickProvider = (settings: Store): Provider => {
  const name = settings.get<string>("ai.provider") ?? "ollama"
  switch (name) {
    case "openai":    return openai({ apiKey: getSecret("openai") })
    case "anthropic": return anthropic({ apiKey: getSecret("anthropic") })
    default:          return ollama({})
  }
}

const summarise = (ai: Provider, text: string) =>
  ai.chat({
    system: "Summarise in one sentence.",
    messages: [{ role: "user", content: text }],
  })
```

Now the user picks the backend in Settings — your code doesn't care.

## API keys

Don't hardcode. Don't put in `@basket/store` (plaintext on disk). Use `@basket/secrets`:

```ts
import { createVault } from "@basket/secrets"

const vault = createVault("io.wess.notes")
await vault.set("openai.apiKey", userInput)

const apiKey = await vault.get("openai.apiKey")
if (!apiKey) {
  toast.error("Set your OpenAI API key in Settings")
  return
}
const ai = openai({ apiKey })
```

## Embeddings

For semantic search over notes:

```ts
import { openai } from "@basket/ai"
import { from } from "@basket/db"

const ai = openai({ apiKey })

// Index
const notesToIndex = db.all(from(notes))
const { vectors } = await ai.embed!({
  input: notesToIndex.map((n) => `${n.title}\n${n.body}`),
})
for (const [i, vec] of vectors.entries()) {
  db.insert(noteEmbeddings, { noteId: notesToIndex[i]!.id, vector: JSON.stringify(vec) })
}

// Search
const { vectors: [qVec] } = await ai.embed!({ input: query })
const all = db.query<{ noteId: number; vector: string }>(
  "SELECT noteId, vector FROM noteEmbeddings",
)
const ranked = all
  .map((r) => ({ noteId: r.noteId, score: cosine(qVec!, JSON.parse(r.vector) as number[]) }))
  .sort((a, b) => b.score - a.score)
  .slice(0, 10)
```

For larger corpora use a vector extension like
[sqlite-vec](https://github.com/asg017/sqlite-vec) — load it with
`db.raw.loadExtension()` and query via raw SQL.

## Local-only with Ollama

Run [Ollama](https://ollama.com) locally; basket talks to it on `http://127.0.0.1:11434`:

```ts
import { ollama } from "@basket/ai"

const ai = ollama({ defaultModel: "llama3.2" })
const reply = await ai.chat({
  messages: [{ role: "user", content: "Hi" }],
})
```

No API key, no network round-trip outside the user's machine. Perfect for offline assistants and privacy-first apps.

## @basket/mcp — expose your app to AI clients

MCP (Model Context Protocol) lets external AI clients (Claude Desktop, Cursor, …) read your app's data and call its actions. Useful especially for "co-working with the user's local data" — search their notes, draft new ones, etc.

Ship an MCP server **alongside** your app:

```ts
// src/mcp.ts
import { createMcpServer } from "@basket/mcp"
import { connect, from } from "@basket/db"
import { paths } from "@basket/config"
import { notes } from "./host/schema"

const db = connect(`${paths({ name: "Notes" }).data}/notes.db`)

const server = createMcpServer({
  name: "notes",
  version: "1.0.0",
  description: "Local Notes app",
})

server.tool({
  name: "list-notes",
  description: "List notes, most recent first",
  handler: () => db.all(from(notes).order("updatedAt", "desc")),
})

server.tool<{ query: string }, unknown>({
  name: "search-notes",
  description: "Search notes by query",
  inputSchema: {
    type: "object",
    properties: { query: { type: "string" } },
    required: ["query"],
  },
  handler: ({ query }) => db.query("SELECT * FROM notes WHERE title LIKE ?", `%${query}%`),
})

server.tool<{ title: string; body?: string }, unknown>({
  name: "create-note",
  description: "Create a new note",
  inputSchema: {
    type: "object",
    properties: { title: { type: "string" }, body: { type: "string" } },
    required: ["title"],
  },
  handler: ({ title, body }) => db.insert(notes, { title, body: body ?? "" }),
})

server.resource({
  uri: "notes://recent",
  description: "10 most recently updated notes (markdown)",
  mimeType: "text/markdown",
  handler: () => {
    const recent = db.all(from(notes).order("updatedAt", "desc").limit(10))
    return recent.map((n) => `# ${n.title}\n\n${n.body}`).join("\n\n---\n\n")
  },
})

await server.serve()
```

Then in Claude Desktop's `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "notes": {
      "command": "bun",
      "args": ["/path/to/notes/src/mcp.ts"]
    }
  }
}
```

Restart Claude Desktop. The notes tools and `notes://recent` resource appear in the picker.

### Coordinating with the running app

Two patterns:

1. **Read-only MCP, write through the main app.** MCP reads the same SQLite file the app writes to. Safe because SQLite WAL handles multi-process reads.
2. **Write via IPC.** MCP makes HTTP requests to a tiny local server you run in the host. The host validates and applies. This avoids SQLite locking edge cases but adds complexity.

For 80% of apps, pattern 1 is fine. The notes db opens with WAL mode by default in `connect()`.

## Streaming + cancellation

Pass an `AbortSignal`:

```ts
const controller = new AbortController()

const streaming = ai.chatStream({ messages, signal: controller.signal })

// later, user clicks "stop"
controller.abort()
```

`fetch` aborts; the `for await` loop terminates with an error you can catch.

## Pricing-aware UI

`reply.usage` (when present) reports token counts. Pair with the
provider's pricing to show a running cost meter, useful for power users.

```ts
const reply = await ai.chat({ messages })
log.info("ai-call", { usage: reply.usage })
```

## Anti-patterns

- **Don't load the API key in the webview.** Keep it host-side. Expose `chat`/`embed` via an IPC channel.
- **Don't stream by collecting and emitting at the end.** Defeats the point. Emit per chunk; the user wants to see tokens arrive.
- **Don't embed every note on every keystroke** — debounce, batch, or cache. Embedding costs.
- **Don't hardcode model strings everywhere.** Centralize in `@basket/store`-backed settings so the user can switch models.

## Next

- [`packages/ai/AGENTS.md`](../packages/ai/AGENTS.md) — full provider API
- [`packages/mcp/AGENTS.md`](../packages/mcp/AGENTS.md) — MCP server details
- [data.md](data.md) — combining AI with local SQLite
