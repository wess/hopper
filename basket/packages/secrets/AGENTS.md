# @basket/secrets

OS keychain access. Stores secrets in the system credential store —
Keychain on macOS, libsecret/secret-tool on Linux, Windows Credential
Manager on Windows. Use for auth tokens, API keys, anything that must
not live in `@basket/store` (which is plain JSON on disk).

## Exports

- `setSecret(service, key, value)` → `Promise<void>`
- `getSecret(service, key)` → `Promise<string | undefined>`
- `deleteSecret(service, key)` → `Promise<void>`
- `createVault(service)` → `Vault` — namespaced helper

## Types

```ts
type Vault = {
  get: (key: string) => Promise<string | undefined>
  set: (key: string, value: string) => Promise<void>
  delete: (key: string) => Promise<void>
  getJson: <T>(key: string) => Promise<T | undefined>
  setJson: (key: string, value: unknown) => Promise<void>
}
```

`service` and `key` are restricted to `^[a-zA-Z0-9._-]+$` (alphanumerics
with `.`, `_`, `-`) to keep shell escaping safe.

## Usage

```ts
import { createVault } from "@basket/secrets"

const vault = createVault("io.wess.notes")

await vault.set("github.token", token)
const t = await vault.get("github.token")  // string | undefined
await vault.delete("github.token")

// JSON convenience
await vault.setJson("oauth.google", { accessToken, refreshToken, expiresAt })
const creds = await vault.getJson<{ accessToken: string }>("oauth.google")
```

## Platform notes

- **macOS**: `security` CLI. Existing entries for the same `service`/`key`
  pair are deleted before set to avoid stacking.
- **Linux**: `secret-tool` from libsecret. The user's keyring must be
  unlocked.
- **Windows**: `cmdkey` for set/delete. **`get` returns `undefined`** —
  `cmdkey` does not expose the stored password value. Apps targeting
  Windows must ship a native helper (e.g. a tiny C++ tool using
  `CredRead`) and shadow these functions, or store the secret elsewhere
  (DPAPI-encrypted file) on Windows.

## Why not @basket/store?

`@basket/store` writes plain JSON to `paths.config`. That's correct for
preferences, window state, recently-opened lists — anything a user
could view in a text editor without security implications. Tokens,
passwords, API keys, OAuth credentials must live in the keychain so:

- they survive OS-level "clear app data" wipes
- they're protected by the user's login session (macOS) / keyring
  unlock (Linux)
- they're not exfiltrated by a malicious app reading your `paths.config`

## Depends on

Nothing. Shells out to platform commands.
