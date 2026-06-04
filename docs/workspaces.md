# Workspaces — design notes

## Problem

A developer's machine runs many unrelated stacks at once: the app they're
working on, its dependencies, plus leftovers from other projects. Docker
Desktop shows one flat, global list. "Show me only the things I care about
right now" is the recurring need. **Workspaces** are saved, named scopes that
filter the UI down to a slice of your Docker resources.

## v1 (implemented)

A workspace is a pure client-side filter — no daemon changes, instant to switch:

```ts
type Workspace = {
  id: string;
  name: string;
  composeProjects?: string[]; // container's compose project ∈ this set
  namePattern?: string;       // regex matched against container/image name
};
```

- A container matches when **every** provided predicate matches (AND). An empty
  workspace matches everything; the built-in **"All resources"** scope (id
  `all`) applies no filter.
- Compose projects are the natural unit of grouping (`com.docker.compose.project`
  label), so the editor offers the detected projects as toggles. The name
  pattern is an escape hatch for non-compose containers.
- Scope is applied in the **Containers** and **Images** views (`matchesWorkspace` /
  `imageMatchesWorkspace`). Images carry no compose project, so only the name
  pattern narrows them.
- The active workspace + the workspace list persist to `localStorage`
  (`hopper:prefs`); switching happens in the sidebar, managing in Settings.

### Why client-side first

The Engine API can filter `containers/json` server-side via `filters={"label":…}`,
but a compose project is one label and a name pattern isn't expressible there.
Filtering the already-fetched list is simpler, instant, and keeps the event-driven
auto-refresh working unchanged. The cost (fetch-all-then-filter) is negligible at
the scale of a dev machine.

## Where this goes next

The richer vision that makes Hopper a true Docker Desktop replacement:

1. **Engine targets / Docker contexts.** A workspace gains an optional
   `dockerHost` (a `DOCKER_HOST` like `ssh://user@host` or a named docker
   context). Switching the workspace then re-points the whole app at a different
   engine — local, a remote server, a VM, a cloud builder. The client already
   centralizes the socket in `host/docker/client.ts` (`DOCKER_SOCKET`); this
   becomes a per-workspace `DOCKER_HOST` the host honors, with the engine
   poller + event stream re-bound on switch. This is the single highest-value
   extension.
2. **Scope everything, not just two views.** Carry the active workspace into the
   Dashboard counts, the activity feed, the tray menu, and prune operations
   ("prune within this workspace").
3. **Server-side label filters.** When a workspace is purely label-based, push
   the filter into the Engine API request to avoid over-fetching on big hosts.
4. **Project-aware actions.** "Start/stop/restart the whole workspace" (a compose
   `up`/`down` across its projects), and per-workspace default run options.
5. **Auto-workspaces.** Offer to create a workspace the first time a new compose
   project appears, so scopes track reality without manual upkeep.

## Open questions

- Should a workspace be allowed to *combine* engine target + filter, or are
  "contexts" (engine) and "workspaces" (filter) two orthogonal switchers? Current
  lean: one switcher, with the engine target as an optional field — fewer
  concepts for the user.
- Persistence: `localStorage` is fine for filters, but engine targets + secrets
  (SSH) belong in the host store / OS keychain (`@basket/secrets`). The model
  will split when `dockerHost` lands.
