import { join } from "node:path";
import { defineConfig } from "@basket/config";
import { emit, handle } from "@basket/ipc";
import { applyMenu, onMenu } from "@basket/menu";
import { createStore } from "@basket/store";
import { mainWindow, setWindow } from "@basket/window";
import { version } from "../../package.json";
import * as ch from "../shared/channels.ts";
import type { DockerEvent, EngineState, EngineStatus } from "../shared/types.ts";
import * as build from "./docker/build.ts";
import * as compose from "./docker/compose.ts";
import * as containers from "./docker/containers.ts";
import * as credentials from "./docker/credentials.ts";
import * as exec from "./docker/exec.ts";
import * as images from "./docker/images.ts";
import * as logs from "./docker/logs.ts";
import * as networks from "./docker/networks.ts";
import * as stats from "./docker/stats.ts";
import * as system from "./docker/system.ts";
import * as volumes from "./docker/volumes.ts";
import {
  engineStatsNow,
  engineStatus,
  reclaimEngine,
  startEngine,
  stopEngine,
} from "./engine/registry.ts";
import { getResources, setResources } from "./engine/resources.ts";
import menu from "./menu.ts";
import * as migrate from "./migrate/index.ts";
import { registerRegistryHandlers } from "./registry/index.ts";
import { startTray } from "./tray.ts";

const config = defineConfig({ app: { name: "Hopper", id: "io.wess.hopper" } });
const settings = createStore("settings", { app: config.app, defaults: { theme: "dark" } });

const win = mainWindow({
  defaults: { width: 1380, height: 880, title: config.app.name },
  store: settings,
  storeKey: "window",
});

// Show the app version in the title bar, e.g. "Hopper v0.1.2". Overrides any
// restored title so it always reflects the running build. version is inlined
// from package.json at compile time.
setWindow({ title: `${config.app.name} v${version}` });

applyMenu(menu);

// --- engine connection + event firehose -----------------------------------
// Ask the active provider for engine status; when it changes, tell the
// webview. While connected, run the event stream — each event refreshes the
// relevant view and feeds the activity log.

let connected = false;
let lastState: EngineState | "" = "";
let lastDetail: string | undefined;
let eventsAbort: AbortController | null = null;

const startEvents = (): void => {
  eventsAbort?.abort();
  const ac = new AbortController();
  eventsAbort = ac;
  void system
    .streamEvents(ac.signal, (e: DockerEvent) => {
      emit(ch.dockerEvent, e);
      if (
        e.type === "container" ||
        e.type === "image" ||
        e.type === "volume" ||
        e.type === "network"
      ) {
        emit(ch.resourcesChanged, { kind: e.type });
      }
    })
    .catch(() => {
      // stream ended (likely a disconnect) — the poller re-establishes it
    });
};

// Broadcast on any state transition (not just connected/disconnected) and
// drive the event firehose as connectivity flips.
const broadcast = (status: EngineStatus): void => {
  // Include detail so a failure reason on an already-seen coarse state still
  // reaches the webview (e.g. two "unreachable" states with different causes).
  if (
    status.state === lastState &&
    status.connected === connected &&
    status.detail === lastDetail
  ) {
    return;
  }
  const wasConnected = connected;
  lastState = status.state;
  lastDetail = status.detail;
  connected = status.connected;
  emit(ch.engineStatusChanged, status);
  if (connected && !wasConnected) startEvents();
  if (!connected && wasConnected) eventsAbort?.abort();
};

const poll = async (): Promise<void> => {
  broadcast(await engineStatus());
};

void poll();
setInterval(() => void poll(), 3000);

// --- engine / system handlers ---------------------------------------------

handle(ch.enginePing, () => engineStatus());
handle(ch.engineStart, async () => {
  const status = await startEngine();
  broadcast(status);
  return status;
});
handle(ch.engineStop, async () => {
  const status = await stopEngine();
  broadcast(status);
  return status;
});
handle(ch.engineReclaim, () => reclaimEngine());
handle(ch.engineStats, () => engineStatsNow());
handle(ch.engineResources, () => getResources());
handle(ch.engineSetResources, (r) => setResources(r));
// Migration (Docker Desktop -> Hopper): scan the source, then run a selected
// migration, streaming per-item progress back to the webview.
handle(ch.migrationScan, () => migrate.scan());
handle(ch.migrationRun, (plan) => migrate.migrate(plan, (p) => emit(ch.migrationProgress, p)));

handle(ch.systemVersion, () => system.version());
handle(ch.systemInfo, () => system.info());
handle(ch.systemDf, () => system.df());
handle(ch.systemPrune, () => system.pruneAll());

// --- container handlers ----------------------------------------------------

handle(ch.containerList, ({ all }) => containers.list(all ?? true));
handle(ch.containerInspect, ({ id }) => containers.inspect(id));
handle(ch.containerStart, ({ id }) => containers.start(id));
handle(ch.containerStop, ({ id }) => containers.stop(id));
handle(ch.containerRestart, ({ id }) => containers.restart(id));
handle(ch.containerPause, ({ id }) => containers.pause(id));
handle(ch.containerUnpause, ({ id }) => containers.unpause(id));
handle(ch.containerKill, ({ id }) => containers.kill(id));
handle(ch.containerRemove, ({ id, force, volumes: v }) => containers.remove(id, force, v));
handle(ch.containerRename, ({ id, name }) => containers.rename(id, name));
handle(ch.containerTop, ({ id }) => containers.top(id));
handle(ch.containerRun, (input) => containers.run(input));
handle(ch.containerPrune, async () => ({ kind: "containers", ...(await containers.prune()) }));

handle(ch.containerBatch, async ({ ids, op }) => {
  await Promise.allSettled(
    ids.map((id) => {
      if (op === "start") return containers.start(id);
      if (op === "stop") return containers.stop(id);
      if (op === "restart") return containers.restart(id);
      return containers.remove(id, true, false);
    }),
  );
});

// --- log streaming ---------------------------------------------------------
// Keyed by container id; the id doubles as the stream's requestId.

const logStreams = new Map<string, AbortController>();

handle(ch.logStart, ({ id, tail }) => {
  logStreams.get(id)?.abort();
  const ac = new AbortController();
  logStreams.set(id, ac);
  void logs
    .stream(id, tail ?? 200, ac.signal, (text, stream) => {
      emit(ch.logLine, { requestId: id, text, stream });
    })
    .catch(() => logStreams.delete(id));
});

handle(ch.logStop, ({ requestId }) => {
  logStreams.get(requestId)?.abort();
  logStreams.delete(requestId);
});

// --- stats streaming -------------------------------------------------------

const statStreams = new Map<string, AbortController>();

handle(ch.statsStart, ({ id }) => {
  statStreams.get(id)?.abort();
  const ac = new AbortController();
  statStreams.set(id, ac);
  void stats
    .stream(id, ac.signal, (s) => emit(ch.statsSample, s))
    .catch(() => statStreams.delete(id));
});

handle(ch.statsStop, ({ id }) => {
  statStreams.get(id)?.abort();
  statStreams.delete(id);
});

// --- exec sessions ---------------------------------------------------------

const execSessions = new Map<string, exec.ExecSession>();
let execSeq = 0;

handle(ch.execStart, async ({ id, cmd }) => {
  const sessionId = `exec-${++execSeq}`;
  const session = await exec.open(id, cmd, {
    onData: (text) => emit(ch.execChunk, { sessionId, text, done: false }),
    onClose: (err) => {
      emit(ch.execChunk, { sessionId, text: "", done: true, error: err });
      execSessions.delete(sessionId);
    },
  });
  execSessions.set(sessionId, session);
  return { sessionId };
});

handle(ch.execInput, ({ sessionId, data }) => {
  execSessions.get(sessionId)?.write(data);
});

handle(ch.execResize, async ({ sessionId, cols, rows }) => {
  await execSessions.get(sessionId)?.resize(cols, rows);
});

handle(ch.execStop, ({ sessionId }) => {
  execSessions.get(sessionId)?.close();
  execSessions.delete(sessionId);
});

// --- image handlers --------------------------------------------------------

handle(ch.imageList, ({ all }) => images.list(all ?? false));
handle(ch.imageInspect, ({ id }) => images.inspect(id));
handle(ch.imageHistory, ({ id }) => images.history(id));
handle(ch.imageRemove, ({ id, force }) => images.remove(id, force));
handle(ch.imageTag, ({ id, repo, tag }) => images.tag(id, repo, tag));
handle(ch.imagePrune, async ({ all }) => ({
  kind: "images",
  ...(await images.prune(all ?? false)),
}));
handle(ch.imageSearch, ({ term }) => images.search(term));
handle(ch.imagePull, ({ requestId, ref }) =>
  images.pull(requestId, ref, (p) => emit(ch.pullProgress, p)),
);
handle(ch.imageBuild, ({ requestId, input }) =>
  build.buildImage(requestId, input, (p) => emit(ch.buildProgress, p)),
);
handle(ch.imagePush, ({ requestId, ref }) =>
  images.push(requestId, ref, (p) => emit(ch.pushProgress, p)),
);

// --- volume handlers -------------------------------------------------------

handle(ch.volumeList, () => volumes.list());
handle(ch.volumeInspect, ({ name }) => volumes.inspect(name));
handle(ch.volumeCreate, ({ name, driver }) => volumes.create(name, driver));
handle(ch.volumeRemove, ({ name, force }) => volumes.remove(name, force));
handle(ch.volumePrune, async () => ({ kind: "volumes", ...(await volumes.prune()) }));

// --- network handlers ------------------------------------------------------

handle(ch.networkList, () => networks.list());
handle(ch.networkInspect, ({ id }) => networks.inspect(id));
handle(ch.networkCreate, (input) => networks.create(input));
handle(ch.networkRemove, ({ id }) => networks.remove(id));
handle(ch.networkPrune, async () => ({ kind: "networks", ...(await networks.prune()) }));
handle(ch.networkConnect, ({ id, container }) => networks.connect(id, container));
handle(ch.networkDisconnect, ({ id, container, force }) =>
  networks.disconnect(id, container, force),
);

// --- registry search (Docker Hub / GHCR / Quay) ----------------------------

registerRegistryHandlers();

handle(ch.registryLogins, () => credentials.listRegistries());

// --- compose ---------------------------------------------------------------
// Bring stacks up/down via the docker compose CLI; output streams by requestId.

handle(ch.composeAvailable, () => compose.available());
handle(ch.composeUp, ({ requestId, file, project }) =>
  compose.runCompose(requestId, "up", file, project, (p) => emit(ch.composeProgress, p)),
);
handle(ch.composeDown, ({ requestId, file, project }) =>
  compose.runCompose(requestId, "down", file, project, (p) => emit(ch.composeProgress, p)),
);

// --- mcp -------------------------------------------------------------------
// The standalone Docker MCP server lives next to this host entry point.

handle(ch.mcpConfig, () => ({
  command: "bun",
  args: [join(import.meta.dir, "..", "mcp.ts")],
}));

// --- tray / status-bar app -------------------------------------------------

startTray(win);

// --- native menu -----------------------------------------------------------

onMenu("view:containers", () => emit(ch.runCommand, "containers"));
onMenu("view:images", () => emit(ch.runCommand, "images"));
onMenu("view:volumes", () => emit(ch.runCommand, "volumes"));
onMenu("view:networks", () => emit(ch.runCommand, "networks"));
onMenu("view:dashboard", () => emit(ch.runCommand, "dashboard"));
onMenu("image:pull", () => emit(ch.runCommand, "pull"));
onMenu("container:run", () => emit(ch.runCommand, "run"));
onMenu("view:palette", () => emit(ch.runCommand, "palette"));
onMenu("theme:toggle", () => emit(ch.runCommand, "theme"));
onMenu("app:settings", () => emit(ch.runCommand, "settings"));
onMenu("system:prune", () => emit(ch.runCommand, "prune"));

onMenu("app:quit", () => {
  win.save();
  process.exit(0);
});

console.log("[Hopper] host ready");
