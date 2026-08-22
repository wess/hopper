---
title: Images
group: Use
order: 2
summary: Local images, registry search across Docker Hub and GitHub, and pulling without a terminal.
---

## The list

Every local image with its tags, size and age. Dangling images are marked, and
`<none>:<none>` renders as a short digest rather than nothing.

**Run** on any row opens the run dialog with the image filled in. **Delete**
removes it, with a force option for images a stopped container still references.

## Finding images

The Registry view searches **Docker Hub** and **GitHub Container Registry** over
HTTP, not through the daemon. That matters for two reasons: it works before an
engine is even up, and it works identically on every backend — Apple's runtime
has no daemon-side search at all.

Results show the description, stars and whether the image is official. **Pull**
brings it down; **Run** pulls and opens the run dialog.

## Credentials

Registry credentials live in the OS keychain under `io.wess.hopper`, one
single-line JSON blob per key. A pull is authenticated whenever a credential
exists for that registry — which is what raises Docker Hub's anonymous rate limit
and unlocks private and GHCR images. With no credential it is an anonymous pull.

## Running one

The run dialog covers name, ports, volumes, environment, network, working
directory, user, resource limits and labels.

On Apple Containers a few of those have nowhere to go — restart policies and
`--hostname` do not exist in that runtime. Hopper does not drop them silently;
it warns that they were not applied.
