import { defineChannel, defineEvent } from "@basket/ipc";
import type {
  BuildInput,
  BuildProgress,
  ComposeProgress,
  Container,
  ContainerStats,
  DiskUsage,
  DockerEvent,
  EngineResources,
  EngineStats,
  EngineStatus,
  ExecChunk,
  GithubStatus,
  Image,
  ImageHistoryEntry,
  ImageSearchResult,
  InspectResult,
  LogLine,
  McpLaunch,
  MigrationPlan,
  MigrationProgress,
  MigrationScan,
  Network,
  NetworkCreateInput,
  ProcessList,
  PruneReport,
  PullProgress,
  PushProgress,
  ReclaimResult,
  RegistryConnection,
  RegistryLoginInput,
  RegistryResult,
  RegistrySource,
  RunInput,
  SystemInfo,
  SystemVersion,
  Volume,
} from "./types.ts";

// --- engine / system ------------------------------------------------------

export const enginePing = defineChannel<void, EngineStatus>("engine:ping");
// Lifecycle for managed providers (our VM, native dockerd) — bring the engine
// up / down from the UI. Both resolve to the post-action status.
export const engineStart = defineChannel<void, EngineStatus>("engine:start");
export const engineStop = defineChannel<void, EngineStatus>("engine:stop");
// Managed-engine maintenance: reclaim disk (fstrim) and live VM stats. Both
// resolve to null/no-op for unmanaged providers.
export const engineReclaim = defineChannel<void, ReclaimResult>("engine:reclaim");
export const engineStats = defineChannel<void, EngineStats | null>("engine:stats");
// Configurable VM resources (cpu/memory/disk). Set returns the normalized
// (clamped) values; changes take effect when the engine next starts.
export const engineResources = defineChannel<void, EngineResources>("engine:resources");
export const engineSetResources = defineChannel<EngineResources, EngineResources>(
  "engine:setResources",
);

// --- migration (Docker Desktop / another engine -> Hopper) ----------------
// Scan the source engine, run a migration for a selection, and stream progress.
export const migrationScan = defineChannel<void, MigrationScan>("migration:scan");
export const migrationRun = defineChannel<MigrationPlan, { ok: boolean; error?: string }>(
  "migration:run",
);
export const migrationProgress = defineEvent<MigrationProgress>("evt:migration");
export const systemVersion = defineChannel<void, SystemVersion>("system:version");
export const systemInfo = defineChannel<void, SystemInfo>("system:info");
export const systemDf = defineChannel<void, DiskUsage>("system:df");
export const systemPrune = defineChannel<void, PruneReport[]>("system:prune");

// Host broadcasts: engine reachability flips, and the live event firehose.
export const engineStatusChanged = defineEvent<EngineStatus>("evt:engine");
export const dockerEvent = defineEvent<DockerEvent>("evt:docker");

// --- containers -----------------------------------------------------------

export const containerList = defineChannel<{ all?: boolean }, Container[]>("containers:list");
export const containerInspect = defineChannel<{ id: string }, InspectResult>("containers:inspect");
export const containerStart = defineChannel<{ id: string }, void>("containers:start");
export const containerStop = defineChannel<{ id: string }, void>("containers:stop");
export const containerRestart = defineChannel<{ id: string }, void>("containers:restart");
export const containerPause = defineChannel<{ id: string }, void>("containers:pause");
export const containerUnpause = defineChannel<{ id: string }, void>("containers:unpause");
export const containerKill = defineChannel<{ id: string }, void>("containers:kill");
export const containerRemove = defineChannel<
  { id: string; force?: boolean; volumes?: boolean },
  void
>("containers:remove");
export const containerRename = defineChannel<{ id: string; name: string }, void>(
  "containers:rename",
);
export const containerTop = defineChannel<{ id: string }, ProcessList>("containers:top");
export const containerRun = defineChannel<RunInput, { id: string }>("containers:run");
export const containerPrune = defineChannel<void, PruneReport>("containers:prune");

// Batch lifecycle actions on a selection (start/stop/restart/remove all).
export const containerBatch = defineChannel<
  { ids: string[]; op: "start" | "stop" | "restart" | "remove" },
  void
>("containers:batch");

// Logs: start streaming a container's logs; the host emits `logLine` events
// tagged by requestId and `logStop` cancels.
export const logStart = defineChannel<{ id: string; tail?: number }, void>("logs:start");
export const logStop = defineChannel<{ requestId: string }, void>("logs:stop");
export const logLine = defineEvent<LogLine>("evt:logline");

// Stats: subscribe to a container's resource stream. The host emits
// `statsSample` events; `statsStop` cancels a subscription.
export const statsStart = defineChannel<{ id: string }, void>("stats:start");
export const statsStop = defineChannel<{ id: string }, void>("stats:stop");
export const statsSample = defineEvent<ContainerStats>("evt:stats");

// --- exec (interactive terminal) ------------------------------------------
// Open a shell in a running container. The host runs the exec session and
// streams output via `execChunk`; the webview pushes keystrokes with
// `execInput` and resizes with `execResize`.
export const execStart = defineChannel<{ id: string; cmd: string[] }, { sessionId: string }>(
  "exec:start",
);
export const execInput = defineChannel<{ sessionId: string; data: string }, void>("exec:input");
export const execResize = defineChannel<{ sessionId: string; cols: number; rows: number }, void>(
  "exec:resize",
);
export const execStop = defineChannel<{ sessionId: string }, void>("exec:stop");
export const execChunk = defineEvent<ExecChunk>("evt:execchunk");

// --- images ---------------------------------------------------------------

export const imageList = defineChannel<{ all?: boolean }, Image[]>("images:list");
export const imageInspect = defineChannel<{ id: string }, InspectResult>("images:inspect");
export const imageHistory = defineChannel<{ id: string }, ImageHistoryEntry[]>("images:history");
export const imageRemove = defineChannel<{ id: string; force?: boolean }, void>("images:remove");
export const imageTag = defineChannel<{ id: string; repo: string; tag: string }, void>(
  "images:tag",
);
export const imagePrune = defineChannel<{ all?: boolean }, PruneReport>("images:prune");
export const imageSearch = defineChannel<{ term: string }, ImageSearchResult[]>("images:search");

// Pull: the host streams progress via `pullProgress` events keyed by
// requestId; resolves when the pull completes (or errors).
export const imagePull = defineChannel<
  { requestId: string; ref: string },
  { ok: boolean; error?: string }
>("images:pull");
export const pullProgress = defineEvent<PullProgress>("evt:pull");

// Build: streams build output via `buildProgress` events keyed by requestId;
// resolves with the built image id (or an error) when the build finishes.
export const imageBuild = defineChannel<
  { requestId: string; input: BuildInput },
  { ok: boolean; error?: string; imageId?: string }
>("images:build");
export const buildProgress = defineEvent<BuildProgress>("evt:build");

// Push: streams progress via `pushProgress` events keyed by requestId.
// Credentials are resolved on the host from the user's docker config.
export const imagePush = defineChannel<
  { requestId: string; ref: string },
  { ok: boolean; error?: string }
>("images:push");
export const pushProgress = defineEvent<PushProgress>("evt:push");

// --- volumes --------------------------------------------------------------

export const volumeList = defineChannel<void, Volume[]>("volumes:list");
export const volumeInspect = defineChannel<{ name: string }, InspectResult>("volumes:inspect");
export const volumeCreate = defineChannel<{ name: string; driver?: string }, Volume>(
  "volumes:create",
);
export const volumeRemove = defineChannel<{ name: string; force?: boolean }, void>(
  "volumes:remove",
);
export const volumePrune = defineChannel<void, PruneReport>("volumes:prune");

// --- networks -------------------------------------------------------------

export const networkList = defineChannel<void, Network[]>("networks:list");
export const networkInspect = defineChannel<{ id: string }, InspectResult>("networks:inspect");
export const networkCreate = defineChannel<NetworkCreateInput, Network>("networks:create");
export const networkRemove = defineChannel<{ id: string }, void>("networks:remove");
export const networkPrune = defineChannel<void, PruneReport>("networks:prune");
export const networkConnect = defineChannel<{ id: string; container: string }, void>(
  "networks:connect",
);
export const networkDisconnect = defineChannel<
  { id: string; container: string; force?: boolean },
  void
>("networks:disconnect");

// --- registry search ------------------------------------------------------
// Unified image search across Docker Hub, GHCR/GitHub, and Quay. `sources`
// narrows which registries to query (defaults to all).
export const registrySearch = defineChannel<
  { term: string; sources?: RegistrySource[] },
  RegistryResult[]
>("registry:search");

// Logged-in registry hosts (read from the user's docker config) — surfaced in
// Settings. Returns hostnames only, never secrets.
export const registryLogins = defineChannel<void, string[]>("registry:logins");

// --- accounts / credentials -----------------------------------------------
// In-app registry sign-in so push/pull work with no `docker login` CLI. Login
// validates against the daemon's /auth and stores the credential in the OS
// keychain; the list and logout manage Hopper-owned credentials.
export const registryLogin = defineChannel<RegistryLoginInput, { ok: boolean; error?: string }>(
  "registry:login",
);
export const registryLogout = defineChannel<{ server: string }, void>("registry:logout");
export const registryConnections = defineChannel<void, RegistryConnection[]>(
  "registry:connections",
);

// GitHub account: connect with a PAT (raises search limits, unlocks private
// repo search + GHCR pulls), disconnect, and report status. Tokens never leave
// the host; status returns the login only.
export const githubConnect = defineChannel<
  { token: string },
  { ok: boolean; login?: string; error?: string }
>("github:connect");
export const githubDisconnect = defineChannel<void, void>("github:disconnect");
export const githubStatus = defineChannel<void, GithubStatus>("github:status");

// --- compose --------------------------------------------------------------
// Bring a compose file up/down by shelling out to the `docker compose` CLI.
// `composeAvailable` gates the feature when the CLI isn't installed. Output
// streams via `composeProgress` events keyed by requestId.
export const composeAvailable = defineChannel<void, boolean>("compose:available");
export const composeUp = defineChannel<
  { requestId: string; file: string; project?: string },
  { ok: boolean; error?: string }
>("compose:up");
export const composeDown = defineChannel<
  { requestId: string; file: string; project?: string },
  { ok: boolean; error?: string }
>("compose:down");
export const composeProgress = defineEvent<ComposeProgress>("evt:compose");

// --- mcp ------------------------------------------------------------------
// The launch command for Hopper's standalone Docker MCP server.
export const mcpConfig = defineChannel<void, McpLaunch>("app:mcpconfig");

// --- host -> webview broadcasts -------------------------------------------

// Any resource changed (fired off the docker event stream) — views refetch.
// `kind` lets a view ignore unrelated churn.
export const resourcesChanged = defineEvent<{ kind: "container" | "image" | "volume" | "network" }>(
  "evt:resources",
);
// A native menu item asked the webview to do something (navigate, run, …).
export const runCommand = defineEvent<string>("evt:command");
// The tray (status-bar app) asked the webview to navigate to a view.
export const trayNavigate = defineEvent<string>("evt:traynav");
