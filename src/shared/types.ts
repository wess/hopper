// The shared data contract between the host and the webview. Every shape that
// crosses IPC lives here. These are lean projections of the Docker Engine API
// — enough to drive the UI, not the full inspect dumps (those travel as
// `InspectResult`, an opaque JSON record rendered generically).

export type InspectResult = Record<string, unknown>;

// --- containers -----------------------------------------------------------

// Docker's lifecycle states, normalized. Drives the status dot + filters.
export type ContainerState =
  | "created"
  | "running"
  | "paused"
  | "restarting"
  | "removing"
  | "exited"
  | "dead";

export type Port = {
  readonly ip?: string;
  readonly privatePort: number;
  readonly publicPort?: number;
  readonly type: string; // tcp | udp | sctp
};

export type Mount = {
  readonly type: string; // bind | volume | tmpfs
  readonly source: string;
  readonly destination: string;
  readonly mode: string;
  readonly rw: boolean;
  readonly name?: string;
};

export type Container = {
  readonly id: string;
  readonly name: string; // primary name, no leading slash
  readonly image: string;
  readonly imageId: string;
  readonly command: string;
  readonly created: number; // unix seconds
  readonly state: ContainerState;
  readonly status: string; // human string, e.g. "Up 3 hours"
  readonly ports: readonly Port[];
  readonly labels: Readonly<Record<string, string>>;
  readonly mounts: readonly Mount[];
  readonly networks: readonly string[];
  // The docker-compose project/service this container belongs to, if any
  // (from `com.docker.compose.*` labels). Drives grouping.
  readonly composeProject: string | null;
  readonly composeService: string | null;
};

// One sampled frame from the stats stream — already reduced to display values.
export type ContainerStats = {
  readonly id: string;
  readonly cpuPercent: number;
  readonly memUsage: number; // bytes
  readonly memLimit: number; // bytes
  readonly memPercent: number;
  readonly netRx: number; // bytes
  readonly netTx: number; // bytes
  readonly blockRead: number; // bytes
  readonly blockWrite: number; // bytes
  readonly pids: number;
};

export type ProcessList = {
  readonly titles: readonly string[];
  readonly processes: readonly (readonly string[])[];
};

// Input for creating + starting a container ("Run" dialog).
export type RunInput = {
  readonly image: string;
  readonly name?: string;
  readonly env?: readonly string[]; // KEY=VALUE
  readonly ports?: readonly {
    readonly host: string;
    readonly container: string;
    readonly proto?: string;
  }[];
  readonly volumes?: readonly {
    readonly host: string;
    readonly container: string;
    readonly ro?: boolean;
  }[];
  readonly command?: string;
  readonly restart?: string; // no | always | unless-stopped | on-failure
  readonly autoRemove?: boolean;
};

// --- images ---------------------------------------------------------------

export type Image = {
  readonly id: string;
  readonly repoTags: readonly string[];
  readonly repoDigests: readonly string[];
  readonly created: number; // unix seconds
  readonly size: number; // bytes
  readonly containers: number; // -1 when unknown
  readonly dangling: boolean;
  readonly labels: Readonly<Record<string, string>>;
};

export type ImageHistoryEntry = {
  readonly id: string;
  readonly created: number;
  readonly createdBy: string;
  readonly size: number;
  readonly comment: string;
};

// One frame of `docker pull` progress, keyed by layer id.
export type PullProgress = {
  readonly requestId: string;
  readonly id?: string;
  readonly status: string;
  readonly current?: number;
  readonly total?: number;
  readonly done: boolean;
  readonly error?: string;
};

// A Docker Hub search result.
export type ImageSearchResult = {
  readonly name: string;
  readonly description: string;
  readonly stars: number;
  readonly official: boolean;
  readonly automated: boolean;
};

// Input for `docker build` (the classic builder). `contextDir` is the build
// context root on the host; `dockerfile` is relative to it.
export type BuildInput = {
  readonly contextDir: string;
  readonly dockerfile?: string; // default "Dockerfile"
  readonly tag?: string; // name:tag to apply to the built image
  readonly target?: string; // multi-stage target stage
  readonly buildArgs?: Readonly<Record<string, string>>;
  readonly noCache?: boolean;
  readonly pull?: boolean; // always attempt to pull a newer base image
};

// One frame of build output. `stream` carries the daemon's build log lines;
// `imageId` is set on the final aux frame when the build succeeds.
export type BuildProgress = {
  readonly requestId: string;
  readonly stream?: string;
  readonly status?: string;
  readonly imageId?: string;
  readonly done: boolean;
  readonly error?: string;
};

// One frame of `docker push` progress, keyed by layer id — same shape family
// as `PullProgress`.
export type PushProgress = {
  readonly requestId: string;
  readonly id?: string;
  readonly status: string;
  readonly current?: number;
  readonly total?: number;
  readonly done: boolean;
  readonly error?: string;
};

// --- volumes --------------------------------------------------------------

export type Volume = {
  readonly name: string;
  readonly driver: string;
  readonly mountpoint: string;
  readonly created: string; // ISO-ish string from the daemon
  readonly scope: string;
  readonly labels: Readonly<Record<string, string>>;
  readonly size: number; // bytes, -1 when unknown (needs df)
  readonly inUse: boolean;
};

// --- networks -------------------------------------------------------------

export type Network = {
  readonly id: string;
  readonly name: string;
  readonly driver: string;
  readonly scope: string;
  readonly internal: boolean;
  readonly attachable: boolean;
  readonly ipam: readonly { readonly subnet?: string; readonly gateway?: string }[];
  readonly containers: number;
  readonly created: string;
  readonly labels: Readonly<Record<string, string>>;
};

export type NetworkCreateInput = {
  readonly name: string;
  readonly driver?: string;
  readonly internal?: boolean;
  readonly attachable?: boolean;
  readonly subnet?: string;
  readonly gateway?: string;
};

// --- system ---------------------------------------------------------------

export type SystemVersion = {
  readonly version: string;
  readonly apiVersion: string;
  readonly os: string;
  readonly arch: string;
  readonly kernelVersion: string;
  readonly goVersion: string;
  readonly buildTime: string;
};

export type SystemInfo = {
  readonly name: string;
  readonly containers: number;
  readonly containersRunning: number;
  readonly containersPaused: number;
  readonly containersStopped: number;
  readonly images: number;
  readonly serverVersion: string;
  readonly operatingSystem: string;
  readonly architecture: string;
  readonly ncpu: number;
  readonly memTotal: number;
  readonly dockerRootDir: string;
};

// `docker system df` — disk usage totals + reclaimable.
export type DiskUsage = {
  readonly images: { readonly count: number; readonly size: number; readonly reclaimable: number };
  readonly containers: {
    readonly count: number;
    readonly size: number;
    readonly reclaimable: number;
  };
  readonly volumes: { readonly count: number; readonly size: number; readonly reclaimable: number };
  readonly buildCache: {
    readonly count: number;
    readonly size: number;
    readonly reclaimable: number;
  };
};

export type DockerEvent = {
  readonly type: string; // container | image | volume | network | ...
  readonly action: string;
  readonly actor: string; // name or id of the subject
  readonly time: number; // unix seconds
  readonly message: string; // pre-formatted one-liner for the activity feed
};

// The engine lifecycle as Hopper sees it. Richer than a reachable/not
// boolean so the UI can act: a managed provider can be started, a stopped
// daemon restarted, a permission problem explained — instead of just told
// to "go open something else".
export type EngineState =
  | "connected" // reachable and answering /_ping
  | "starting" // a managed provider is bringing the engine up (or first probe in flight)
  | "stopped" // an engine is present/known but not currently listening
  | "notInstalled" // no engine and no provider that can supply one here
  | "unreachable" // the endpoint exists but errors/times out
  | "needsPermission" // the socket is there but we're denied (e.g. docker group)
  | "unsupported"; // the active provider can't run on this machine

// Whether the daemon is reachable, and enough context to do something about
// it. `connected` is kept as a derived convenience (=== state "connected").
export type EngineStatus = {
  readonly state: EngineState;
  readonly connected: boolean;
  readonly message: string; // human one-liner (also feeds the activity banner)
  readonly provider: string; // active provider id ("vz" | "linux" | "existing" | …)
  readonly managed: boolean; // does Hopper own this engine's lifecycle?
  readonly detail?: string; // extra diagnostic line for the UI
  readonly endpoint?: string; // human description of where we're talking
};

// Configurable VM resources for a managed engine. CPU + memory apply when the
// engine (re)starts; disk size applies to a freshly-created data disk.
export type EngineResources = {
  readonly cpus: number;
  readonly memoryGiB: number;
  readonly diskGiB: number;
};

// Live stats from a managed engine's VM (from the in-guest agent).
export type EngineStats = {
  readonly memTotalKb: number;
  readonly memAvailKb: number;
  readonly diskTotalKb: number;
  readonly diskUsedKb: number;
  readonly load1: number;
};

// Result of a disk-reclaim request against a managed engine.
export type ReclaimResult = {
  readonly ok: boolean;
  readonly detail: string;
};

// --- migration (another engine, e.g. Docker Desktop -> Hopper) -------------

// A serializable Docker connection target — mirrors host/docker/endpoint.ts's
// Endpoint exactly so it round-trips through IPC. The scan resolves the source
// engine and the plan pins it, so a daemon coming up/down between scan and run
// can't silently redirect the migration to a different engine.
export type MigrationEndpoint =
  | { readonly transport: "unix"; readonly path: string }
  | { readonly transport: "npipe"; readonly path: string }
  | {
      readonly transport: "tcp";
      readonly host: string;
      readonly port: number;
      readonly tls: boolean;
    };

// One migratable object discovered on the source engine.
export type MigrationItem = {
  readonly kind: "image" | "volume" | "network" | "container";
  readonly id: string; // image id / volume name / network id / container id
  readonly name: string; // repo:tag / volume name / network name / container name
  readonly detail?: string; // size, status, or backing image
};

// What the source engine holds, surfaced for selection.
export type MigrationScan = {
  readonly available: boolean; // a migratable source engine was found
  readonly source?: string; // human description of the source endpoint
  readonly sourceEndpoint?: MigrationEndpoint; // the resolved source, pinned into the plan
  readonly containers: readonly MigrationItem[];
  readonly images: readonly MigrationItem[];
  readonly volumes: readonly MigrationItem[];
  readonly networks: readonly MigrationItem[];
  readonly message?: string;
};

// The user's selection of what to migrate (ids per kind) + the pinned source.
export type MigrationPlan = {
  readonly source?: MigrationEndpoint; // the exact engine the selection came from
  readonly containers: readonly string[];
  readonly images: readonly string[];
  readonly volumes: readonly string[];
  readonly networks: readonly string[];
};

// Streamed migration progress.
export type MigrationProgress = {
  readonly phase: "networks" | "volumes" | "images" | "containers" | "done" | "error";
  readonly item: string; // current object's name ("" between items)
  readonly done: number; // items completed in this phase
  readonly total: number; // items in this phase
  readonly message: string;
  readonly error?: string; // non-fatal per-item error (migration continues)
  readonly warning?: string; // non-fatal advisory (e.g. host-path bind, arch mismatch)
  readonly finished?: boolean; // the whole migration is complete
};

// A pruned-resource report (containers/images/volumes/networks/buildcache).
export type PruneReport = {
  readonly kind: string;
  readonly removed: number;
  readonly reclaimed: number; // bytes
};

// --- log / exec streaming -------------------------------------------------

export type LogLine = {
  readonly requestId: string;
  readonly text: string;
  readonly stream: "stdout" | "stderr";
};

// A chunk of output from an interactive exec session, keyed by sessionId.
export type ExecChunk = {
  readonly sessionId: string;
  readonly text: string;
  readonly done: boolean;
  readonly error?: string;
};

// --- registry search ------------------------------------------------------
// A unified image-search result across registries. `ref` is a ready-to-pull
// reference (e.g. "nginx", "ghcr.io/owner/app", "quay.io/org/img"). `url` is
// the human web page for the result.
export type RegistrySource = "dockerhub" | "ghcr" | "quay";

export type RegistryResult = {
  readonly source: RegistrySource;
  readonly name: string; // display name (repo/owner)
  readonly ref: string; // pullable reference
  readonly description: string;
  readonly stars: number; // popularity signal (-1 when n/a)
  readonly official: boolean;
  readonly url: string; // web page
  readonly updated?: string; // ISO last-updated, when known
};

// --- accounts / credentials -----------------------------------------------
// In-app registry sign-in (no `docker login` CLI needed). Credentials are kept
// in the OS keychain via @basket/secrets and consulted by push/pull.
export type RegistryLoginInput = {
  readonly server?: string; // registry host; blank/"docker.io" => Docker Hub
  readonly username: string;
  readonly password: string; // password or access token / PAT
};

// One logged-in registry, for the Accounts panel. `source` says whether the
// credential lives in Hopper's keychain or in the user's existing docker config
// (the latter is read-only here — we surface it but don't own it).
export type RegistryConnection = {
  readonly server: string; // canonical server address
  readonly label: string; // friendly name ("Docker Hub", host, …)
  readonly username?: string; // known for keychain creds; absent for helpers
  readonly source: "hopper" | "docker";
};

// GitHub connection: a stored PAT raises search rate limits, unlocks private
// repo search, and authenticates GHCR pulls.
export type GithubStatus = {
  readonly connected: boolean;
  readonly login?: string; // the authenticated account, when connected
};

// --- compose --------------------------------------------------------------
// One line of output from a `docker compose up/down` run, streamed to the UI.
// `done` marks the final frame; `error` is set when the run exited non-zero.
export type ComposeProgress = {
  readonly requestId: string;
  readonly line: string;
  readonly stream: "stdout" | "stderr";
  readonly done: boolean;
  readonly error?: string;
};

// Aggregate status of a compose stack.
export type ComposeStackStatus = "running" | "partial" | "stopped";

// One container instance belonging to a compose service (a service may have
// several when scaled).
export type ComposeService = {
  readonly service: string; // compose service name
  readonly containerId: string;
  readonly containerName: string;
  readonly state: ContainerState;
  readonly status: string; // human "Up 3 hours"
  readonly image: string;
  readonly ports: readonly Port[];
};

// A compose project (stack), reconstructed from container labels so it works
// against any engine without a running compose CLI.
export type ComposeProject = {
  readonly name: string;
  readonly status: ComposeStackStatus;
  readonly running: number;
  readonly total: number;
  readonly services: readonly ComposeService[];
  readonly configFiles: readonly string[]; // com.docker.compose.project.config_files
  readonly workingDir: string | null; // com.docker.compose.project.working_dir
};

// A lifecycle action over a whole stack. `remove` is a full teardown
// (down + volumes + orphans).
export type ComposeAction = "up" | "down" | "start" | "stop" | "restart" | "remove";

// Which files / project / env the compose command targets. Lifecycle ops on an
// existing stack (start/stop/restart/down) work label-driven with just a
// project; `up` needs the file(s).
export type ComposeTarget = {
  readonly files?: readonly string[]; // -f (repeatable)
  readonly project?: string; // -p
  readonly envFile?: string; // --env-file
};

// Extra flags for up/down.
export type ComposeOptions = {
  readonly profiles?: readonly string[]; // --profile (repeatable)
  readonly build?: boolean; // up --build
  readonly forceRecreate?: boolean; // up --force-recreate
  readonly removeOrphans?: boolean; // up/down --remove-orphans (up defaults true)
  readonly volumes?: boolean; // down --volumes
  readonly rmi?: "all" | "local"; // down --rmi
};

// Result of validating a compose file set (`docker compose config`).
export type ComposeConfigResult = {
  readonly ok: boolean;
  readonly yaml?: string; // normalized, merged config on success
  readonly error?: string; // validation error on failure
};

// Reading/writing a compose file on the host filesystem (the in-app editor).
export type ComposeFileResult = {
  readonly ok: boolean;
  readonly content?: string;
  readonly error?: string;
};

// --- MCP ------------------------------------------------------------------
// The launch command for Hopper's standalone Docker MCP server, surfaced in
// Settings so users can register it with an AI client.
export type McpLaunch = {
  readonly command: string;
  readonly args: readonly string[];
};

// --- workspaces -----------------------------------------------------------
// A workspace is a saved, named scope over your Docker resources — pick the
// compose projects and/or a name pattern you care about and the UI filters to
// just those. The built-in "all" workspace (empty predicates) shows everything.
// A container matches a workspace when every provided predicate matches (AND).
export type Workspace = {
  readonly id: string;
  readonly name: string;
  readonly composeProjects?: readonly string[]; // container's compose project must be one of these
  readonly namePattern?: string; // regex tested against container/image name
};
