# @basket/request

`fetch`-based HTTP client with retries, interceptors, timeouts, and JSON auto-handling.

## Exports

- `createClient(options)` → `Client` — configured client
- `isRequestError(e)` — type guard for errors thrown on non-2xx responses
- `RequestError` (type) — `Error & { status; response; data }`

## Types

```ts
type ClientOptions = {
  baseURL?: string
  headers?: Record<string, string>
  timeout?: number              // ms
  retries?: number              // default 0
  retryDelay?: (attempt) => number   // default exp backoff capped at 30s
  retryOn?: (status, error?) => boolean   // default: 5xx + 429
  requestInterceptors?:  ((init) => init | Promise<init>)[]
  responseInterceptors?: ((res)  => res  | Promise<res>)[]
}

type Client = {
  request: <T>(init) => Promise<Response<T>>
  get:     <T>(url, init?)             => Promise<Response<T>>
  post:    <T>(url, body?, init?)      => Promise<Response<T>>
  put:     <T>(url, body?, init?)      => Promise<Response<T>>
  patch:   <T>(url, body?, init?)      => Promise<Response<T>>
  delete:  <T>(url, init?)             => Promise<Response<T>>
}

type Response<T> = { status; headers; data: T; raw: globalThis.Response }
```

## Usage

```ts
import { createClient, RequestError } from "@basket/request"

const api = createClient({
  baseURL: "https://api.example.com",
  headers: { authorization: "Bearer xxx" },
  retries: 3,
  timeout: 15_000,
})

const { data: notes } = await api.get<Note[]>("/notes")
const { data: created } = await api.post<Note>("/notes", { title: "Hi" })

try {
  await api.get("/private")
} catch (e) {
  if (isRequestError(e) && e.status === 401) {
    // refresh token, retry, etc.
  }
}
```

### Interceptors

```ts
const api = createClient({
  baseURL: "https://api.example.com",
  requestInterceptors: [
    async (init) => ({
      ...init,
      headers: { ...init.headers, authorization: `Bearer ${await getToken()}` },
    }),
  ],
  responseInterceptors: [
    (res) => {
      log.info({ status: res.status, url: res.raw.url })
      return res
    },
  ],
})
```

### Query params + abort

```ts
const controller = new AbortController()
setTimeout(() => controller.abort(), 5_000)

await api.get("/search", {
  query: { q: term, limit: 50 },
  signal: controller.signal,
})
```

## Retries

By default the client retries on HTTP 5xx and 429, with exponential
backoff (100ms, 200ms, 400ms, …, capped at 30s). Network errors
(`fetch` rejection) are also retried unless `retryOn(0, err)` returns
false. `AbortError` is not retried.

## Body handling

- `string`, `FormData`, `Blob`, `ArrayBuffer` → sent verbatim
- Anything else → `JSON.stringify` + `content-type: application/json`

Responses are auto-parsed: `content-type: application/json` → `JSON.parse`,
otherwise `text()`.

## Depends on

Nothing. Uses the global `fetch` and `AbortController`.
