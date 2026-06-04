# @basket/ai

Functional providers for OpenAI, Anthropic, and local Ollama. Same
`Provider` interface — pick one or swap at runtime.

## Exports

- `openai({ apiKey, baseURL?, defaultModel?, defaultEmbedModel? })` → `Provider`
- `anthropic({ apiKey, baseURL?, defaultModel?, version? })` → `Provider`
- `ollama({ baseURL?, apiKey?, defaultModel?, defaultEmbedModel? })` → `Provider`

## Types

```ts
type Role = "system" | "user" | "assistant"
type Message = { role: Role; content: string }

type ChatRequest = {
  messages: readonly Message[]
  model?: string
  temperature?: number
  maxTokens?: number
  system?: string
  signal?: AbortSignal
}

type ChatResponse = {
  content: string
  model: string
  usage?: { promptTokens?: number; completionTokens?: number }
  raw: unknown
}

type ChatChunk = { delta: string; done: boolean }

type EmbedRequest = { input: string | readonly string[]; model?: string }
type EmbedResponse = { vectors: number[][]; model: string; raw: unknown }

type Provider = {
  name: string
  chat: (req: ChatRequest) => Promise<ChatResponse>
  chatStream: (req: ChatRequest) => AsyncIterable<ChatChunk>
  embed?: (req: EmbedRequest) => Promise<EmbedResponse>
}
```

## Usage

```ts
import { anthropic, openai, ollama } from "@basket/ai"

const ai = anthropic({ apiKey: Bun.env.ANTHROPIC_API_KEY ?? "" })

const reply = await ai.chat({
  system: "You are a helpful assistant.",
  messages: [{ role: "user", content: "Summarise this note in one sentence." }],
})
console.log(reply.content)
```

### Streaming

```ts
for await (const chunk of ai.chatStream({ messages: [...] })) {
  if (chunk.delta) process.stdout.write(chunk.delta)
  if (chunk.done) break
}
```

### Embeddings

```ts
const ai = openai({ apiKey: Bun.env.OPENAI_API_KEY ?? "" })
const { vectors } = await ai.embed!({ input: ["hello world", "another"] })
```

Embeddings supported by `openai` and `ollama`. `anthropic` doesn't ship
an embed API — use a separate provider for vectors.

### Local-only with Ollama

```ts
const ai = ollama({ defaultModel: "llama3.2" })
```

Talks to `http://127.0.0.1:11434` — the user must have Ollama running
locally. Great for offline assistants.

### Ollama Cloud

Point `baseURL` at the cloud host and pass an `apiKey` to use Ollama's
hosted models instead of a local server. The `apiKey` becomes a
`Bearer` token; a local server ignores it.

```ts
const ai = ollama({
  baseURL: "https://ollama.com",
  apiKey: Bun.env.OLLAMA_API_KEY ?? "",
  defaultModel: "gpt-oss:120b",
})
```

## Sourcing the API key

Don't hardcode API keys. Read from env (`Bun.env`) or — better — from
`@basket/secrets` so the key is stored in the OS keychain:

```ts
import { createVault } from "@basket/secrets"
const vault = createVault("io.wess.notes")
const apiKey = await vault.get("anthropic.apiKey")
if (!apiKey) throw new Error("Set your Anthropic API key in Settings")
const ai = anthropic({ apiKey })
```

## Default models

| Provider | Chat | Embed |
|---|---|---|
| OpenAI | `gpt-4o-mini` | `text-embedding-3-small` |
| Anthropic | `claude-sonnet-4-6` | — |
| Ollama | `llama3.2` | `nomic-embed-text` |

Override per call via `req.model` or globally via `defaultModel` /
`defaultEmbedModel`.

## Depends on

Nothing. Uses `fetch`, `crypto`, and `AsyncIterable`.
