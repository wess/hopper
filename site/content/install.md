---
title: Install
group: Start
order: 1
summary: Download the signed app, or install it with Homebrew.
---

## Homebrew

```sh
brew install --cask wess/packages/hopper
```

## Download

Grab `Hopper.dmg` from the [latest release](https://github.com/wess/hopper/releases/latest),
open it, and drag **Hopper.app** to Applications.

The build is signed with a Developer ID and notarized, so Gatekeeper opens it
without complaint. If you want to confirm that yourself:

```sh
spctl -a -t exec -vv /Applications/Hopper.app
# accepted
# source=Notarized Developer ID
```

## What Hopper needs

| | |
|---|---|
| **Architecture** | Apple Silicon (arm64) |
| **macOS** | Sonoma or later to run the app |
| **For Apple Containers** | macOS 26 or later |

The split matters. Hopper itself runs on Sonoma quite happily as a client for an
engine you already have — Docker Desktop, Colima, Rancher Desktop, or a remote
daemon. What needs macOS 26 is *Apple's* container runtime, because the vmnet
APIs that give containers their own addresses only exist there.

Hopper does not block the install over that. It tells you in-app which engines
this machine can actually run.

## Hopper asks for very little

Three entitlements, and no more:

- `files.user-selected.read-write` — bind-mounted directories, and the tars an
  import stages on disk
- `network.client` and `network.server` — registry pulls and published ports

Notably **not** `com.apple.security.virtualization`. Hopper used to run a Linux
VM of its own and needed it; Apple's runtime does its own virtualizing under its
own privileged helpers, so the entitlement is gone and the release build asserts
it has not come back.
