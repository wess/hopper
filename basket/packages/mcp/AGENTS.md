# @basket/mcp

Expose app state and actions as Model Context Protocol tools so AI
clients (Claude Desktop, Cursor, etc.) can read from and act on your
desktop app.

## Exports

- `createMcpServer({ name, version, description? })` → `BasketMcpServer`

Server methods (chainable):
- `.tool({ name, description?, inputSchema?, handler })` — expose an action
- `.resource({ uri, name?, description?, mimeType?, handler })` — expose readable content
- `.serve()` → `Promise<void>` — start the server over stdio

## Types

```ts
type ToolDef<I, O> = {
  name: string
  description?: string
  inputSchema?: Record<string, unknown>   // JSON Schema
  handler: (input: I) => O | Promise<O>
}

type ResourceDef = {
  uri: string                              // e.g. "notes://recent"
  name?: string
  description?: string
  mimeType?: string                        // default "text/plain"
  handler: () => string | Promise<string>
}
```

## Usage

A common pattern: ship an MCP server **alongside** your app that calls
the same business logic. Run it via `bun run mcp.ts`.

```ts
// src/mcp.ts
import { createMcpServer } from "@basket/mcp"
import { connect, from } from "@basket/db"
import { paths } from "@basket/config"
import { notesTable } from "./host/schema"

const db = connect(`${paths({ name: "Notes" }).data}/notes.db`)

const server = createMcpServer({
  name: "notes",
  version: "1.0.0",
})

server.tool({
  name: "list-notes",
  description: "List all notes",
  handler: () => db.all(from(notesTable).order("updatedAt", "desc")),
})

server.tool<{ title: string; body?: string }, unknown>({
  name: "create-note",
  description: "Create a new note",
  inputSchema: {
    type: "object",
    properties: {
      title: { type: "string" },
      body: { type: "string" },
    },
    required: ["title"],
  },
  handler: ({ title, body }) => db.insert(notesTable, { title, body: body ?? "" }),
})

server.resource({
  uri: "notes://recent",
  description: "10 most recently updated notes (markdown)",
  mimeType: "text/markdown",
  handler: () => {
    const recent = db.all(from(notesTable).order("updatedAt", "desc").limit(10))
    return recent.map((n) => `# ${n.title}\n\n${n.body}`).join("\n\n---\n\n")
  },
})

await server.serve()
```

Then in Claude Desktop's MCP config:

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

## Stdio transport

The default transport is stdio — the MCP client launches your script as
a subprocess and talks over stdin/stdout. That's why MCP servers must
never `console.log` to stdout for anything other than protocol frames
— use `@basket/logger` to write to a file instead.

## Tool results

Whatever the handler returns is serialized:
- Strings pass through verbatim
- Everything else is `JSON.stringify`'d with 2-space indentation
- The result is wrapped in a `{ type: "text", text }` content block

For richer responses (images, multi-part content), drop down to the
`@modelcontextprotocol/sdk` directly — `createMcpServer` is a thin sugar
layer for the common case.

## Depends on

- `@modelcontextprotocol/sdk` — the MCP TypeScript SDK
