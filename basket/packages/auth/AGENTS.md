# @basket/auth

Desktop-app authentication primitives: Argon2id password hashing, OAuth
2.1 *client* flows with PKCE, and keychain-backed session storage.

## Exports

- `hashPassword(pw)` → `Promise<string>` — Argon2id via `Bun.password`
- `verifyPassword(pw, hash)` → `Promise<boolean>`
- `createPkce()` → `{ verifier, challenge, method: "S256" }`
- `randomState(length?)` → URL-safe random string
- `openBrowser(url)` — open `url` in the user's default browser
- `createOAuthClient(options)` → `OAuthClient` — PKCE auth-code flow
- `createSession<T>(options)` → `Session<T>` — keychain-backed session storage

## OAuth flow

`createOAuthClient(opts).start()`:

1. Generate PKCE `verifier` + `challenge` and a random `state`
2. Build the authorize URL with `code_challenge`, `redirect_uri`, etc.
3. Open the system browser
4. Start a tiny `Bun.serve` on `127.0.0.1:<port>` listening for the callback
5. On callback: verify `state`, POST `tokenUrl` with `code` + `verifier`
6. Resolve with `TokenResponse`; close server, show "you can close this window" HTML

`refresh(refreshToken)` uses the `refresh_token` grant.

## Types

```ts
type OAuthOptions = {
  authUrl: string
  tokenUrl: string
  clientId: string
  clientSecret?: string         // confidential clients only
  scopes: readonly string[]
  redirectPort?: number         // default 53682
  redirectPath?: string         // default "/callback"
  extraAuthParams?: Record<string, string>
}

type TokenResponse = {
  accessToken: string
  refreshToken?: string
  expiresIn?: number
  tokenType?: string
  scope?: string
  raw: Record<string, unknown>
}

type Session<T> = {
  get: () => Promise<T | undefined>
  set: (value: T) => Promise<void>
  clear: () => Promise<void>
  isSignedIn: () => Promise<boolean>
}
```

## Usage

### Local password (e.g. lock-screen for a notes app)

```ts
import { hashPassword, verifyPassword } from "@basket/auth"

const hash = await hashPassword(userInput)
db.update(users, { id }, { passwordHash: hash })

const ok = await verifyPassword(attempt, hash)
```

### GitHub OAuth

```ts
import { createOAuthClient, createSession } from "@basket/auth"

const github = createOAuthClient({
  authUrl: "https://github.com/login/oauth/authorize",
  tokenUrl: "https://github.com/login/oauth/access_token",
  clientId: "Iv1.xxxxxxxxxxxxxxxx",
  clientSecret: "shhh",   // GitHub requires this even for desktop apps
  scopes: ["read:user", "user:email"],
})

const session = createSession<{ token: string; refresh?: string; user: { login: string } }>({
  service: "io.wess.notes",
  key: "github",
})

// sign in
const tok = await github.start()
const me = await fetch("https://api.github.com/user", {
  headers: { authorization: `Bearer ${tok.accessToken}` },
}).then((r) => r.json())
await session.set({ token: tok.accessToken, refresh: tok.refreshToken, user: me })

// later
if (await session.isSignedIn()) { ... }

// sign out
await session.clear()
```

### Refresh

```ts
const current = await session.get()
if (current?.refresh) {
  const next = await github.refresh(current.refresh)
  await session.set({ ...current, token: next.accessToken, refresh: next.refreshToken ?? current.refresh })
}
```

## Why a loopback redirect

OAuth 2.1 for native apps mandates either:

- A custom URL scheme (`myapp://oauth/callback`) — basket supports this
  via `@basket/protocol`, but requires butter bundle config and
  per-platform registration
- A `127.0.0.1` loopback redirect — works with zero setup; this is
  basket's default

The loopback `Bun.serve` runs only during sign-in and shuts down
immediately after the callback resolves. No long-running server, no
firewall prompts (loopback only).

## Depends on

- `@basket/secrets` — for `createSession()`
- `Bun.password` — for Argon2id
- `Bun.serve` + `fetch` — for the OAuth dance
