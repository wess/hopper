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

Grab `Hopper.dmg` from the [latest release](https://github.com/wess/hopper/releases/latest). Open it
and drag **Hopper.app** to Applications.

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
| **Architecture** | Apple silicon (arm64) |
| **macOS** | 26 or later |

Apple's container runtime needs macOS 26 for the vmnet APIs that give containers
their own addresses. Hopper can also attach to Docker Desktop, Podman, Colima,
Rancher Desktop, or a remote daemon from a supported Mac.

## Hopper asks for very little

Three entitlements, and no more:

- `files.user-selected.read-write` — bind-mounted directories, and the tars an
  import stages on disk
- `network.client` and `network.server` — registry pulls and published ports

Notably **not** `com.apple.security.virtualization`. Hopper used to run a Linux
VM of its own and needed it; Apple's runtime does its own virtualizing under its
own privileged helpers, so the entitlement is gone and the release build asserts
it has not come back.
