# Quickstart

Build a working notes-style desktop app in ~80 lines.

## Vendor basket

```bash
curl -sL https://github.com/wess/basket/archive/refs/heads/main.zip -o /tmp/basket.zip
unzip -q /tmp/basket.zip -d /tmp/basket-expand
mv /tmp/basket-expand/basket-main ./basket
rm -rf /tmp/basket.zip /tmp/basket-expand
```

Or, if you already have basket cloned and want to scaffold a project:

```bash
bunx --bun ./basket/packages/cli/entry.ts init myapp --template minimal
cd myapp
```

## package.json

```json
{
  "name": "myapp",
  "module": "src/host/index.ts",
  "type": "module",
  "private": true,
  "workspaces": ["basket/packages/*"],
  "dependencies": {
    "butter": "npm:butterframework@latest",
    "@basket/config": "workspace:*",
    "@basket/db": "workspace:*",
    "@basket/store": "workspace:*",
    "@basket/window": "workspace:*",
    "@basket/ipc": "workspace:*",
    "@basket/menu": "workspace:*"
  },
  "devDependencies": { "@types/bun": "latest" },
  "scripts": {
    "dev": "butter dev",
    "build": "butter compile",
    "bundle": "butter bundle"
  }
}
```

Add `basket/` and `node_modules/` to `.gitignore`.

## butter.yaml

```yaml
window:
  title: "MyApp"
  width: 1000
  height: 700

build:
  entry: src/app/index.html
  host: src/host/index.ts

bundle:
  identifier: com.example.myapp
```

## src/shared/channels.ts

Define IPC channels once; host and webview both import them.

```ts
import { defineChannel, defineEvent } from "@basket/ipc"

export const listTodos = defineChannel<void, Todo[]>("todos:list")
export const addTodo = defineChannel<{ text: string }, Todo>("todos:add")
export const todoAdded = defineEvent<Todo>("todos:added")

export type Todo = { id: number; text: string; done: boolean; createdAt: string }
```

## src/host/index.ts

```ts
import { mkdir } from "node:fs/promises"
import { defineConfig, paths } from "@basket/config"
import { column, connect, defineTable, from, migrate, type RowOf } from "@basket/db"
import { handle, emit } from "@basket/ipc"
import { applyMenu, item, onMenu, section } from "@basket/menu"
import { createStore } from "@basket/store"
import { mainWindow } from "@basket/window"
import { addTodo, listTodos, todoAdded } from "../shared/channels.ts"

const config = defineConfig({ app: { name: "MyApp", id: "com.example.myapp" } })

const todos = defineTable("todos", {
  id: column.serial().primaryKey(),
  text: column.text(),
  done: column.boolean().default(false),
  createdAt: column.timestamp().default("now()"),
})

const dataDir = paths(config.app).data
await mkdir(dataDir, { recursive: true })
const db = connect(`${dataDir}/app.db`)
migrate(db, [todos])

const settings = createStore("settings", { app: config.app })
const win = mainWindow({ defaults: { width: 1000, height: 700 }, store: settings })

applyMenu([section("File", [item("Quit", "quit", { shortcut: "CmdOrCtrl+Q" })])])
onMenu("quit", () => { win.save(); db.close(); process.exit(0) })

handle(listTodos, () => db.all(from(todos).order("createdAt", "desc")) as RowOf<typeof todos>[])
handle(addTodo, ({ text }) => {
  const created = db.insert(todos, { text }) as RowOf<typeof todos>
  emit(todoAdded, created)
  return created
})
```

## src/app/main.ts

```ts
import { invoke, subscribe } from "@basket/ipc/client"
import { addTodo, listTodos, todoAdded } from "../shared/channels.ts"

const list = document.getElementById("list") as HTMLUListElement
const input = document.getElementById("input") as HTMLInputElement

const render = (todos: Awaited<ReturnType<typeof invoke<typeof listTodos>>>) => {
  list.innerHTML = ""
  for (const t of todos) {
    const li = document.createElement("li")
    li.textContent = t.text
    list.append(li)
  }
}

let todos = await invoke(listTodos, undefined as unknown as void)
render(todos)

subscribe(todoAdded, (t) => { todos = [t, ...todos]; render(todos) })

input.addEventListener("keydown", async (e) => {
  if (e.key === "Enter" && input.value.trim()) {
    await invoke(addTodo, { text: input.value.trim() })
    input.value = ""
  }
})
```

## Run

```bash
bun install
bun run dev
```

A native window opens. Type into the input → press Enter → todo persists in SQLite under `~/Library/Application Support/MyApp/app.db` (macOS) and renders live via `subscribe(todoAdded, …)`.

## What you got

- Typed end-to-end IPC: changing a channel's input/output is a type error on both sides
- SQLite with WAL mode, declarative schema, typed query builder
- Window position/size persisted across launches
- Native app menu with keyboard shortcuts
- All under Bun + butter, no Electron, no node_modules bloat
