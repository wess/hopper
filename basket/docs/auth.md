# Auth Guide

`@basket/auth` covers three patterns desktop apps need:

1. **Local passwords** (Argon2id) — lock screens, encrypted vaults, multi-account
2. **OAuth 2.1 client** with PKCE — sign in with GitHub / Google / etc.
3. **Keychain-backed sessions** — survive app restarts, OS reboots

It pairs with [`@basket/secrets`](../packages/secrets/AGENTS.md) for the
keychain layer.

## 1. Local passwords

For a lock screen on a personal app, or for hashing the master password on an encrypted vault:

```ts
import { hashPassword, verifyPassword } from "@basket/auth"

// Sign up
const hash = await hashPassword(userInput)
db.insert(users, { email, passwordHash: hash })

// Sign in
const user = db.one(from(users).where((q) => q("email").equals(email)))
if (!user || !(await verifyPassword(attempt, user.passwordHash))) {
  throw unauthorized("Wrong email or password")
}
```

Backed by `Bun.password.hash(pw, { algorithm: "argon2id" })`. No salt parameter — Argon2id generates and embeds the salt in the hash.

## 2. OAuth client with PKCE

Most desktop apps shouldn't run an OAuth server; they're clients. Sign-in flows look like:

1. App opens the provider's auth URL in the user's system browser
2. User signs in there
3. Provider redirects to a URL the app controls
4. App captures the code, exchanges it for a token
5. App stores the token in the keychain

For step 3, basket uses a **loopback redirect** to `http://127.0.0.1:<port>/callback`. A tiny `Bun.serve` runs only during sign-in:

```ts
import { createOAuthClient } from "@basket/auth"

const github = createOAuthClient({
  authUrl:  "https://github.com/login/oauth/authorize",
  tokenUrl: "https://github.com/login/oauth/access_token",
  clientId: "Iv1.xxxxxxxxxxxxxxxx",
  clientSecret: "shhh",                          // GitHub requires this
  scopes: ["read:user", "user:email"],
  redirectPort: 53682,                           // default
})

const token = await github.start()
// → { accessToken, refreshToken, expiresIn, tokenType, scope, raw }
```

What `start()` does:

1. Generates a PKCE `code_verifier` + `code_challenge` (S256)
2. Generates a random `state`
3. Builds the authorize URL with `code_challenge`, `redirect_uri=http://127.0.0.1:<port>/callback`, etc.
4. Spawns `Bun.serve` on `127.0.0.1:<port>`
5. Opens the browser via `open` / `xdg-open` / `start`
6. Waits for the callback
7. Verifies `state`, POSTs `tokenUrl` with `code` + `code_verifier`
8. Returns the `TokenResponse`, shuts down the server, shows a "you can close this window" page

### Refresh tokens

```ts
const next = await github.refresh(current.refreshToken!)
```

Uses the `refresh_token` grant. Some providers rotate refresh tokens —
always overwrite both:

```ts
await session.set({
  accessToken: next.accessToken,
  refreshToken: next.refreshToken ?? current.refreshToken,
})
```

### Custom auth params

Some providers need extras (`prompt=consent`, `access_type=offline`):

```ts
const google = createOAuthClient({
  authUrl:  "https://accounts.google.com/o/oauth2/v2/auth",
  tokenUrl: "https://oauth2.googleapis.com/token",
  clientId: "...",
  scopes: ["openid", "email", "profile"],
  extraAuthParams: { access_type: "offline", prompt: "consent" },
})
```

## 3. Keychain sessions

After OAuth, store the result so the next launch finds the user signed in:

```ts
import { createSession } from "@basket/auth"

type GithubSession = {
  accessToken: string
  refreshToken?: string
  user: { login: string; id: number }
}

const session = createSession<GithubSession>({
  service: "io.wess.notes",
  key: "github",     // default: "session"
})

// after sign-in
const token = await github.start()
const user = await fetch("https://api.github.com/user", {
  headers: { authorization: `Bearer ${token.accessToken}` },
}).then((r) => r.json())

await session.set({
  accessToken: token.accessToken,
  refreshToken: token.refreshToken,
  user,
})

// next launch
if (await session.isSignedIn()) {
  const me = await session.get()
  // …
}

// sign out
await session.clear()
```

Sessions are serialized to JSON and stored in the OS keychain via
`@basket/secrets`. Survive app reinstalls (on macOS, the keychain entry
persists across reinstalls of the same `service` identifier).

## Putting it together — a sign-in flow

```ts
import { defineConfig, ensurePaths } from "@basket/config"
import { createOAuthClient, createSession } from "@basket/auth"
import { defineChannel, handle, unauthorized } from "@basket/ipc"

const config = defineConfig({ app: { name: "Notes", id: "io.wess.notes" } })
await ensurePaths(config.app)

const github = createOAuthClient({
  authUrl:  "https://github.com/login/oauth/authorize",
  tokenUrl: "https://github.com/login/oauth/access_token",
  clientId: Bun.env.GITHUB_CLIENT_ID!,
  clientSecret: Bun.env.GITHUB_CLIENT_SECRET!,
  scopes: ["read:user"],
})

type Me = { login: string; id: number }

const session = createSession<{ token: string; refresh?: string; user: Me }>({
  service: config.app.id!,
  key: "github",
})

// shared channels
import { signinChannel, signoutChannel, meChannel } from "../shared/auth"

handle(signinChannel, async () => {
  const tok = await github.start()
  const me = (await fetch("https://api.github.com/user", {
    headers: { authorization: `Bearer ${tok.accessToken}` },
  }).then((r) => r.json())) as Me
  await session.set({ token: tok.accessToken, refresh: tok.refreshToken, user: me })
  return me
})

handle(signoutChannel, async () => {
  await session.clear()
  return { ok: true }
})

handle(meChannel, async () => {
  const s = await session.get()
  if (!s) throw unauthorized()
  return s.user
})
```

From the webview:

```ts
import { invoke, isIpcInvocationError } from "@basket/ipc/client"
import { signinChannel, meChannel } from "../shared/auth"

try {
  const me = await invoke(meChannel, undefined as unknown as void)
  renderSignedIn(me)
} catch (e) {
  if (isIpcInvocationError(e) && e.code === "unauthorized") {
    // show a "Sign in with GitHub" button
    onClick("#signin", async () => {
      const me = await invoke(signinChannel, undefined as unknown as void)
      renderSignedIn(me)
    })
  }
}
```

## Refresh tokens at runtime

Refresh before the access token expires:

```ts
const refreshIfNeeded = async () => {
  const s = await session.get()
  if (!s) throw unauthorized()
  // assume `expiresAt` was stored alongside; or test the token with a probe call
  if (Date.now() < (s.expiresAt ?? 0) - 60_000) return s
  if (!s.refresh) throw unauthorized()
  const next = await github.refresh(s.refresh)
  const merged = {
    ...s,
    token: next.accessToken,
    refresh: next.refreshToken ?? s.refresh,
    expiresAt: Date.now() + (next.expiresIn ?? 3600) * 1000,
  }
  await session.set(merged)
  return merged
}
```

Wrap your API client to call `refreshIfNeeded()` first.

## Multi-account

Need to sign in with two GitHub accounts? Use different session keys:

```ts
const sessionA = createSession({ service: config.app.id!, key: "github.personal" })
const sessionB = createSession({ service: config.app.id!, key: "github.work" })
```

Each is independent in the keychain.

## Custom URL scheme alternative

Loopback redirect (`http://127.0.0.1:53682/callback`) is the default
because it requires zero setup. The alternative is a custom URL scheme
(`myapp://oauth/callback`) — register it in `butter.yaml#bundle.urlSchemes` and use `@basket/protocol` to receive the deep link.

Most providers accept loopback. Microsoft Entra ID and a few others
prefer custom schemes for desktop apps. See [the OAuth 2.1 draft, §
8.4](https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1#section-8.4)
for the security rationale.

## Anti-patterns

- **Don't store tokens in `@basket/store`.** Plaintext JSON. Use `@basket/secrets` via `createSession`.
- **Don't reuse a Bun.serve port across runs.** `createOAuthClient` already shuts down its loopback after the callback — don't start your own.
- **Don't ship a `clientSecret` for a public OAuth app**. Use PKCE-only flows (no secret) where the provider supports them; GitHub does *not* and requires the secret, but you should ship a per-installation registration anyway if your distribution model allows.
- **Don't await `start()` without a UI affordance** — it can take 30+ seconds while the user types their password. Show a spinner; let them cancel via an `AbortController` passed to a future `start({ signal })` (planned).

## Next

- [data.md](data.md) — pairing auth with a local user table
- [ipc.md](ipc.md) — `requireAuth` pipe pattern
- [`packages/auth/AGENTS.md`](../packages/auth/AGENTS.md) — full API
- [`packages/secrets/AGENTS.md`](../packages/secrets/AGENTS.md) — keychain details
