# Registry push + credentials — design notes

## Scope

Push a tagged local image to its registry, with live per-layer progress, and
show which registries the user is logged into.

## Credentials: reuse, don't store

The deliberate decision here is that **Hopper stores no secrets of its own.** It
reads the user's existing Docker setup (`~/.docker/config.json`, honoring
`$DOCKER_CONFIG`) and reuses whatever `docker login` already configured, so push
inherits the exact auth the CLI uses.

`src/host/docker/credentials.ts` resolves auth for a ref in this order:

1. A **credential helper** — `credHelpers[host]` or the global `credsStore` — is
   invoked as `docker-credential-<helper> get` with the server on stdin. This is
   the secure path (Keychain on macOS, etc.); the secret never touches disk in
   plaintext. The `<token>` sentinel username is mapped to an `identitytoken`.
2. A plaintext **`auths[host].auth`** entry (`base64(user:pass)`), decoded by
   `decodeAuthEntry()`.
3. Otherwise **anonymous** — the daemon still requires an `X-Registry-Auth`
   header on push, so we send one carrying just the server address.

`registryHost()` maps a ref to its server: bare names and `docker.io/…` →
`https://index.docker.io/v1/` (Hub's canonical key), otherwise the first path
segment when it looks like a host (has a dot, a port, or is `localhost`).
`matchKey()` compares config keys scheme/slash-insensitively so `https://host/`
and `host` match.

## Push

`images.push()` mirrors `pull`: `splitRef()` separates the push name from the
tag (careful not to mistake a `host:port` for a tag), resolves auth, sets the
base64 `X-Registry-Auth` header, and streams `pushProgress` frames per layer.

`registry:logins` lists logged-in registry **hostnames** for Settings — names
only, never secrets.

## Why this shape

Building our own credential store would mean a second source of truth, a
plaintext-secret risk, and a login flow to maintain. Reading the docker config
gives correct behavior for free and keeps Hopper's security surface tiny. An
in-app `docker login` (validate via `POST /auth`, persist through the platform
keychain) is a possible follow-up, but is intentionally not in v1.
