//! `Host` — one method per user-facing operation.
//!
//! Views never touch the Docker client directly; they call through here, which
//! keeps retry, status classification, and workspace scoping in one place.

use docker::client::Client;
use docker::{archive, containers, exec, images, logs, networks, stats, system, volumes};
use model::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[cfg(target_os = "macos")]
use crate::runtime::refuse;
use crate::runtime::Backend;
// `model::*` below brings a `compose` module of its own into scope, so the
// crate of the same name needs the leading `::` to be reachable.
use ::compose::{plan_dir, plan_files, PlanOptions};

pub struct Host {
    client: Client,
    engines: crate::engine::Engines,
    provider: RwLock<String>,
    managed: RwLock<bool>,
    /// Which client answers. Set when an engine is selected; every operation
    /// resolves its backend from this.
    runtime: RwLock<RuntimeKind>,
    /// Selection changes both the registry and the shared client endpoint.
    /// Serialize them so launch, polling, and a user choice cannot race and
    /// leave the UI describing one daemon while requests go to another.
    selection: tokio::sync::Mutex<()>,
    /// Bumped after the shared client and backend have moved to a provider.
    /// Long-lived streams use it to notice that their connection is stale.
    selection_generation: AtomicU64,
    /// Event watching must not begin against the client's construction-time
    /// endpoint while the initial provider selection is still in flight.
    selected: tokio::sync::watch::Sender<bool>,
    selection_events: tokio::sync::watch::Sender<u64>,
    /// Cached Engine API version for the active provider. Connectivity is
    /// probed frequently; version negotiation is only needed once per engine
    /// selection, not on every status tick.
    engine_version: RwLock<Option<String>>,
    settings: RwLock<Settings>,
    workspaces: RwLock<Vec<Workspace>>,
    persistence: std::sync::Mutex<()>,
    directory: std::path::PathBuf,
    pub(crate) inventory: Arc<crate::inventory::Inventory>,
    admitted: bool,
    idle: Arc<crate::idle::Idle>,
}

impl Host {
    pub fn new(client: Client) -> Arc<Self> {
        Self::with_directory(client, store::paths::root())
    }

    /// Explicit state root for isolated callers and verification.
    pub fn with_directory(client: Client, directory: std::path::PathBuf) -> Arc<Self> {
        let mut settings: Settings = store::json::read_or_default(&directory.join("settings.json"));
        // Older or hand-edited settings must not create an invalid VM on the
        // first start after an upgrade.
        settings.resources = settings
            .resources
            .bounded_to(::engine::vm::resources::capacity());
        Arc::new(Self {
            engines: crate::engine::Engines::new(client.clone()),
            client,
            provider: RwLock::new("existing".into()),
            managed: RwLock::new(false),
            runtime: RwLock::new(RuntimeKind::default()),
            selection: tokio::sync::Mutex::new(()),
            selection_generation: AtomicU64::new(0),
            selected: tokio::sync::watch::channel(false).0,
            selection_events: tokio::sync::watch::channel(0).0,
            engine_version: RwLock::new(None),
            settings: RwLock::new(settings),
            workspaces: RwLock::new(store::json::read_or_default(
                &directory.join("workspaces.json"),
            )),
            directory,
            inventory: Arc::new(crate::inventory::Inventory::default()),
            admitted: false,
            idle: Arc::new(crate::idle::Idle::default()),
            persistence: std::sync::Mutex::new(()),
        })
    }

    pub fn from_env() -> Arc<Self> {
        Self::new(Client::from_env())
    }

    pub fn client(&self) -> Client {
        self.client.snapshot()
    }

    /// Point the whole app at a different daemon.
    pub async fn set_endpoint(&self, ep: docker::Endpoint, provider: &str, managed: bool) {
        let _selection = self.selection.lock().await;
        self.client.set_endpoint(ep);
        *self.provider.write().unwrap() = provider.to_string();
        *self.managed.write().unwrap() = managed;
        *self.engine_version.write().unwrap() = None;
        *self.runtime.write().unwrap() = RuntimeKind::EngineApi;
        self.idle.reset();
        self.inventory.invalidate();
        let generation = self.selection_generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.selection_events.send_replace(generation);
        self.selected.send_replace(true);
    }

    /// The client that answers for the active engine.
    pub(crate) fn backend(&self) -> Backend {
        Backend::resolve(*self.runtime.read().unwrap())
    }

    /// Capture backend and connection together while selection is stable.
    /// Requests then run without blocking engine selection and cannot be
    /// redirected by a change to the application's shared client.
    pub(crate) async fn session(&self) -> (Backend, Client) {
        let _selection = self.selection.lock().await;
        (self.backend(), self.client.snapshot())
    }

    async fn inventory_session(&self) -> (Backend, Client, String) {
        let _selection = self.selection.lock().await;
        (
            self.backend(),
            self.client.snapshot(),
            format!(
                "{}:{}",
                self.selection_generation(),
                self.inventory.revision()
            ),
        )
    }

    /// Pin a multi-step operation, including nested facade calls. Settings and
    /// workspace scope are captured once alongside the engine session.
    pub(crate) async fn operation(&self) -> Self {
        let _selection = self.selection.lock().await;
        Self {
            client: self.client.snapshot(),
            engines: self.engines.clone(),
            provider: RwLock::new(self.provider.read().unwrap().clone()),
            managed: RwLock::new(*self.managed.read().unwrap()),
            runtime: RwLock::new(self.runtime_kind()),
            selection: tokio::sync::Mutex::new(()),
            selection_generation: AtomicU64::new(self.selection_generation()),
            selected: tokio::sync::watch::channel(true).0,
            selection_events: tokio::sync::watch::channel(self.selection_generation()).0,
            engine_version: RwLock::new(self.engine_version.read().unwrap().clone()),
            settings: RwLock::new(self.settings()),
            workspaces: RwLock::new(self.workspaces()),
            directory: self.directory.clone(),
            inventory: Arc::clone(&self.inventory),
            admitted: self.admitted,
            idle: Arc::clone(&self.idle),
            persistence: std::sync::Mutex::new(()),
        }
    }

    fn engine_key(&self) -> String {
        format!(
            "{:?}:{}",
            self.runtime_kind(),
            self.client.endpoint().describe()
        )
    }

    pub async fn recent_operations(&self) -> docker::Result<Vec<OperationRecord>> {
        let operation = self.operation().await;
        crate::operations::recent(&operation.directory, &operation.engine_key())
    }

    pub(crate) async fn admit(
        &self,
        action: &str,
    ) -> docker::Result<(Self, Option<crate::operations::Guard>)> {
        let mut operation = self.operation().await;
        self.idle.reset();
        let guard = if self.admitted {
            None
        } else {
            Some(crate::operations::begin(
                &operation.directory,
                &operation.engine_key(),
                action,
            )?)
        };
        operation.admitted = true;
        Ok((operation, guard))
    }

    /// What the active engine can do, so the UI can hide what it cannot.
    pub fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::for_runtime(*self.runtime.read().unwrap())
    }

    pub fn runtime_kind(&self) -> RuntimeKind {
        *self.runtime.read().unwrap()
    }

    // engine

    pub fn engines(&self) -> &crate::engine::Engines {
        &self.engines
    }

    /// Choose an engine and point the client at it.
    pub async fn select_engine(&self) -> EngineStatus {
        let _selection = self.selection.lock().await;
        let before = (
            self.provider.read().unwrap().clone(),
            self.client.endpoint(),
            self.runtime_kind(),
        );
        let preference = self.settings().engine_preference;
        let mut status = self.engines.select(preference.as_deref()).await;
        *self.provider.write().unwrap() = status.provider.clone();
        *self.managed.write().unwrap() = status.managed;
        *self.runtime.write().unwrap() = self.engines.registry().active_runtime();
        *self.engine_version.write().unwrap() = None;
        if status.connected && self.runtime_kind() == RuntimeKind::EngineApi {
            // Initial list requests must use the negotiated API too, rather
            // than waiting for the next status poll to discover an old daemon.
            let negotiated = async {
                self.client.negotiate().await?;
                system::version(&self.client).await
            }
            .await;
            match negotiated {
                Ok(version) => *self.engine_version.write().unwrap() = Some(version.version),
                Err(error) => {
                    status = crate::status::classify(
                        &error,
                        &status.provider,
                        status.managed,
                        &self.client.endpoint().describe(),
                    )
                }
            }
        }
        let after = (
            status.provider.clone(),
            self.client.endpoint(),
            self.runtime_kind(),
        );
        if before != after || !self.selection_ready() {
            self.inventory.invalidate();
            let generation = self.selection_generation.fetch_add(1, Ordering::AcqRel) + 1;
            self.selection_events.send_replace(generation);
        }
        self.selected.send_replace(true);
        status
    }

    /// Generation of the currently selected provider, for long-lived work.
    pub fn selection_generation(&self) -> u64 {
        self.selection_generation.load(Ordering::Acquire)
    }

    pub fn selection_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.selection_events.subscribe()
    }

    /// Wait until the app's initial provider selection has completed.
    ///
    /// This is intentionally separate from [`Self::stream_events`]: library
    /// callers may legitimately use the client's construction-time endpoint
    /// without going through the app startup sequence.
    pub async fn wait_for_initial_selection(&self) {
        let mut selected = self.selected.subscribe();
        while !*selected.borrow_and_update() {
            if selected.changed().await.is_err() {
                break;
            }
        }
    }

    /// Whether the app's first provider selection has finished.
    ///
    /// Views use this synchronous check during construction to avoid issuing
    /// requests against the client's construction-time endpoint. The root
    /// bumps its refresh epoch immediately after selection, which starts the
    /// deferred loads against the selected backend.
    pub fn selection_ready(&self) -> bool {
        *self.selected.borrow()
    }

    pub(crate) async fn lock_selection(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.selection.lock().await
    }

    /// The engines the user can choose between in Settings.
    pub async fn engine_choices(&self) -> Vec<EngineChoice> {
        self.engines.choices().await
    }

    /// Pin the engine to one provider, or hand selection back to Hopper.
    ///
    /// Persisted before re-selecting, because `select_engine` reads the saved
    /// setting — and returns the new status so the caller does not have to
    /// wait for the next poll to see what it got.
    pub async fn set_engine_preference(&self, id: Option<String>) -> Result<EngineStatus, String> {
        let mut settings = self.settings();
        settings.engine_preference = id;
        self.save_settings(settings).map_err(|e| e.to_string())?;
        Ok(self.select_engine().await)
    }

    pub async fn start_engine(&self) -> anyhow::Result<()> {
        // Pin the provider without holding selection through a long boot.
        // Stop, including the quit hook, must be able to cancel that boot.
        let (provider, settings, guard) = {
            let _selection = self.selection.lock().await;
            (
                self.engines
                    .registry()
                    .active()
                    .ok_or_else(|| anyhow::anyhow!("No engine is selected."))?,
                self.settings(),
                crate::operations::begin(&self.directory, &self.engine_key(), "engine_start")?,
            )
        };
        let result = provider.start_configured(&settings).await;
        guard.finish(result.is_ok());
        self.inventory.invalidate();
        result
    }

    pub fn engine_capacity(&self) -> EngineCapacity {
        ::engine::vm::resources::capacity()
    }

    pub fn daemon_document(&self) -> Result<String, String> {
        let document =
            ::engine::vm::daemon::document(&self.settings().daemon).map_err(|e| e.to_string())?;
        serde_json::to_string_pretty(&document).map_err(|e| e.to_string())
    }

    pub async fn engine_stats(&self) -> Option<EngineStats> {
        let _selection = self.selection.lock().await;
        self.engines.registry().active()?.stats().await
    }

    pub async fn reclaim_engine(&self) -> ReclaimResult {
        let (provider, guard) = {
            let _selection = self.selection.lock().await;
            let Some(provider) = self.engines.registry().active() else {
                return ReclaimResult {
                    ok: false,
                    detail: "No engine is selected.".into(),
                };
            };
            let guard = match crate::operations::begin(
                &self.directory,
                &self.engine_key(),
                "engine_reclaim",
            ) {
                Ok(guard) => guard,
                Err(error) => {
                    return ReclaimResult {
                        ok: false,
                        detail: error.message,
                    }
                }
            };
            (provider, guard)
        };
        let result = provider.reclaim().await;
        guard.finish(result.ok);
        result
    }

    pub async fn stop_engine(&self) -> anyhow::Result<()> {
        let (provider, guard) = {
            let _selection = self.selection.lock().await;
            let provider = self
                .engines
                .registry()
                .active()
                .ok_or_else(|| anyhow::anyhow!("No engine is selected."))?;
            // Stop is also cancellation for this process's active startup.
            // That startup owns admission and records its cancelled outcome.
            let guard = if provider.starting() {
                None
            } else {
                Some(crate::operations::begin(
                    &self.directory,
                    &self.engine_key(),
                    "engine_stop",
                )?)
            };
            (provider, guard)
        };
        let result = provider.stop().await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        self.idle.reset();
        self.inventory.invalidate();
        result
    }

    /// Install Hopper's Docker CLI tools for the current user and point them
    /// at the active Engine API endpoint. Existing non-Hopper files are never
    /// replaced.
    #[cfg(target_os = "macos")]
    pub async fn configure_docker_cli(&self) -> DockerCliSetupResult {
        if self.runtime_kind() == RuntimeKind::Apple {
            return DockerCliSetupResult {
                detail: "Apple Containers does not publish a Docker-compatible socket.".into(),
                ..Default::default()
            };
        }
        let host = self.client.endpoint().docker_host_value();
        match tokio::task::spawn_blocking(move || crate::dockercli::configure(&host)).await {
            Ok(result) => result,
            Err(error) => DockerCliSetupResult {
                detail: format!("Docker CLI setup did not complete: {error}"),
                ..Default::default()
            },
        }
    }

    /// Reconcile host port forwarding for a backend that needs it.
    ///
    /// Docker-compatible daemons and Apple's runtime bind published ports
    /// themselves, so this currently has no work. Keep the seam for a future
    /// provider rather than doing anything on every event until one exists.
    pub async fn resync_forwards(&self) -> Vec<String> {
        self.engines.resync_forwards().await
    }

    /// What the UI's status timer calls.
    ///
    /// A plain probe is enough while an engine is answering. While none is,
    /// selection is re-run instead: an engine that appears after launch —
    /// Docker Desktop started by hand, Apple's runtime finishing its install —
    /// has to be picked up without restarting Hopper. Selection is also what
    /// falls forward from a missing Docker to the engine Hopper can supply, so
    /// re-running it is what keeps that offer current in both directions.
    pub async fn poll_engine(&self) -> EngineStatus {
        let status = self.engine_status().await;
        if status.connected && self.suspend_empty_vm().await {
            return self.engine_status().await;
        }
        if status.connected || status.state == model::EngineState::Starting {
            return status;
        }
        self.select_engine().await
    }

    async fn suspend_empty_vm(&self) -> bool {
        let _selection = self.selection.lock().await;
        let settings = self.settings();
        let Some(minutes) = settings
            .idle_suspend_minutes
            .filter(|_| self.engines.active_id() == "vm")
        else {
            self.idle.reset();
            return false;
        };
        let client = self.client.snapshot();
        client.set_timeout(Duration::from_secs(3));
        // Even stopped containers prevent suspension. This deliberately avoids
        // interrupting logs, terminals, paused workloads, or expected restarts.
        if !matches!(containers::list(&client, true).await, Ok(list) if list.is_empty()) {
            self.idle.reset();
            return false;
        }
        if !self
            .idle
            .due(Duration::from_secs(u64::from(minutes.clamp(1, 1440)) * 60))
        {
            return false;
        }
        let Ok(guard) =
            crate::operations::begin(&self.directory, &self.engine_key(), "idle_suspend")
        else {
            self.idle.reset();
            return false;
        };
        let _pause = self.idle.pause();
        if !self.idle.drained().await {
            return false;
        }
        let Some(provider) = self.engines.registry().active() else {
            return false;
        };
        if !provider.idle().await
            || !matches!(containers::list(&client, true).await, Ok(list) if list.is_empty())
        {
            guard.finish(false);
            return false;
        }
        // Check API clients once more after the final inventory connection
        // closes. New external CLI requests during shutdown need a manual
        // engine start; the UI describes that behavior when opting in.
        if !provider.idle().await {
            guard.finish(false);
            return false;
        }
        let stopped = provider.stop().await.is_ok();
        guard.finish(stopped);
        self.inventory.invalidate();
        stopped
    }

    /// Probe the engine and describe what we found.
    pub async fn engine_status(&self) -> EngineStatus {
        // a VM boot holds selection stable, but its progress must remain visible
        if self.engines.starting() {
            return self.engines.status().await;
        }
        // Keep a status probe from racing a provider switch. The client is
        // shared by all providers, so changing its endpoint while a ping or
        // version negotiation is in flight could report the wrong daemon.
        let _selection = self.selection.lock().await;
        // Apple's runtime answers no socket, so there is nothing to ping —
        // its provider reports on the services instead.
        if self.runtime_kind() == RuntimeKind::Apple {
            return self.engines.status().await;
        }
        let vm_probed = self.engines.active_id() == "vm";
        if vm_probed {
            let status = self.engines.status().await;
            if !status.connected {
                *self.engine_version.write().unwrap() = None;
                return status;
            }
        }

        let provider = self.provider.read().unwrap().clone();
        let managed = *self.managed.read().unwrap();
        let endpoint = self.client.endpoint();
        let described = endpoint.describe();

        // Health checks run every three seconds, so a dead or half-open
        // daemon must not consume the normal 30-second operation deadline or
        // leave a pile of probes behind. The selected client remains at its
        // normal timeout for user operations.
        if !vm_probed {
            let probe = Client::new(endpoint);
            probe.set_timeout(Duration::from_secs(3));
            if let Err(e) = probe.ping().await {
                *self.engine_version.write().unwrap() = None;
                return crate::status::classify(&e, &provider, managed, &described);
            }
        }
        let cached_version = { self.engine_version.read().unwrap().clone() };
        let version = if let Some(version) = cached_version {
            version
        } else {
            // Negotiate once we know the daemon is up, so an older engine still
            // gets a version we can both speak. Cache the result: this method
            // runs from the UI's three-second connectivity poll.
            if let Err(e) = self.client.negotiate().await {
                return crate::status::classify(&e, &provider, managed, &described);
            }
            let version = match system::version(&self.client).await {
                Ok(version) => version.version,
                Err(e) => return crate::status::classify(&e, &provider, managed, &described),
            };
            *self.engine_version.write().unwrap() = Some(version.clone());
            version
        };
        crate::status::connected(&provider, managed, &described, &version)
    }

    pub async fn version(&self) -> docker::Result<SystemVersion> {
        let (_backend, client) = self.session().await;
        match _backend {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::system::version(&cli).await,
            Backend::EngineApi => system::version(&client).await,
        }
    }

    pub async fn info(&self) -> docker::Result<SystemInfo> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = _backend {
            // Apple publishes no `info` equivalent, so the dashboard's totals
            // are counted from the lists it does publish.
            // Preserve failures: an engine that went away between the health
            // poll and this request must not look like a healthy empty one.
            let containers = apple::containers::list(&cli, true).await?;
            let images = apple::images::list(&cli).await?;
            let running = containers
                .iter()
                .filter(|c| c.state == ContainerState::Running)
                .count() as i64;
            let version = apple::system::version(&cli).await?;
            return Ok(SystemInfo {
                name: "Apple Containers".into(),
                containers: containers.len() as i64,
                containers_running: running,
                // Apple's runtime cannot pause, so this is always zero rather
                // than unknown.
                containers_paused: 0,
                containers_stopped: containers.len() as i64 - running,
                images: images.len() as i64,
                server_version: version.version,
                operating_system: "macOS".into(),
                architecture: std::env::consts::ARCH.into(),
                ncpu: std::thread::available_parallelism()
                    .map(|n| n.get() as i64)
                    .unwrap_or(0),
                mem_total: 0,
                docker_root_dir: String::new(),
            });
        }
        system::info(&client).await
    }

    pub async fn disk_usage(&self) -> docker::Result<DiskUsage> {
        let (_backend, client) = self.session().await;
        match _backend {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::system::df(&cli).await,
            Backend::EngineApi => system::df(&client).await,
        }
    }

    pub async fn prune_all(&self) -> Vec<PruneReport> {
        let (operation, guard) = match self.admit("prune").await {
            Ok(admission) => admission,
            Err(error) => {
                return vec![PruneReport {
                    kind: "all".into(),
                    error: Some(error.message),
                    ..Default::default()
                }]
            }
        };
        let result = operation.prune_all_bound().await;
        if let Some(guard) = guard {
            guard.finish(result.iter().all(|report| report.error.is_none()));
        }
        result
    }

    async fn prune_all_bound(&self) -> Vec<PruneReport> {
        let _changed = self.inventory.mutation();
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            // No single prune command, so run the four and keep whichever
            // succeeded — one failing kind must not cost the others.
            let mut out = Vec::new();
            for (kind, result) in [
                ("containers", self.containers_prune().await),
                ("images", self.images_prune(false).await),
                ("volumes", self.volumes_prune().await),
                ("networks", self.networks_prune().await),
            ] {
                match result {
                    Ok(report) => out.push(report),
                    Err(error) => out.push(PruneReport {
                        kind: kind.into(),
                        removed: 0,
                        reclaimed: 0,
                        error: Some(error.message),
                    }),
                }
            }
            return out;
        }
        system::prune_all(&client).await
    }

    // containers

    /// List containers, scoped to the active workspace.
    pub async fn containers(&self, all: bool) -> docker::Result<Vec<Container>> {
        let (_backend, client, key) = self.inventory_session().await;
        let list = self.inventory.containers[usize::from(all)]
            .get(key, async {
                Ok(match _backend {
                    #[cfg(target_os = "macos")]
                    Backend::Apple(cli) => apple::containers::list(&cli, all).await?,
                    Backend::EngineApi => containers::list(&client, all).await?,
                })
            })
            .await?;
        let ws = self.active_workspace();
        Ok(list
            .into_iter()
            .filter(|c| matches_workspace(c, ws.as_ref()))
            .collect())
    }

    pub async fn container_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        let (_backend, client) = self.session().await;
        match _backend {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::inspect(&cli, id).await,
            Backend::EngineApi => containers::inspect(&client, id).await,
        }
    }

    pub async fn container_top(&self, id: &str) -> docker::Result<ProcessList> {
        let (_backend, client) = self.session().await;
        match _backend {
            #[cfg(target_os = "macos")]
            Backend::Apple(_) => refuse("list the processes in a container"),
            Backend::EngineApi => containers::top(&client, id).await,
        }
    }

    // container filesystem (the Files tab)

    /// List a directory inside a container.
    pub async fn container_ls(&self, id: &str, dir: &str) -> docker::Result<Vec<FileEntry>> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("browse a container's filesystem");
        }
        archive::list_dir(&client, id, dir).await
    }

    /// Read a file's bytes out of a container.
    pub async fn container_read(&self, id: &str, path: &str) -> docker::Result<Vec<u8>> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("browse a container's filesystem");
        }
        archive::read_file(&client, id, path).await
    }

    /// Export a path from a container to a tar file on the host.
    pub async fn container_export(
        &self,
        id: &str,
        path: &str,
        dest: &std::path::Path,
    ) -> docker::Result<()> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("export a path from a container");
        }
        archive::export_to(&client, id, path, dest).await
    }

    // interactive exec (the Terminal tab)

    /// Start an interactive shell session in a container.
    pub async fn exec_start<F>(
        &self,
        id: &str,
        shell: Option<&str>,
        tty: bool,
        on_output: F,
    ) -> docker::Result<exec::Session>
    where
        F: FnMut(String) -> bool + Send + 'static,
    {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("open a shell in a container");
        }
        exec::start(&client, id, shell, tty, on_output).await
    }

    /// Apply one action to many containers, reporting per-item failures rather
    /// than aborting the batch. Stopping twelve containers must not stop at
    /// the first one that is already down.
    pub async fn container_batch(
        &self,
        ids: &[String],
        action: BatchAction,
    ) -> Vec<(String, Option<String>)> {
        let (operation, guard) = match self.admit("container_batch").await {
            Ok(admission) => admission,
            Err(error) => {
                return ids
                    .iter()
                    .map(|id| (id.clone(), Some(error.message.clone())))
                    .collect()
            }
        };
        let result = operation.container_batch_bound(ids, action).await;
        if let Some(guard) = guard {
            guard.finish(result.iter().all(|(_, error)| error.is_none()));
        }
        result
    }

    async fn container_batch_bound(
        &self,
        ids: &[String],
        action: BatchAction,
    ) -> Vec<(String, Option<String>)> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            let result = match action {
                BatchAction::Start => self.container_start(id).await,
                BatchAction::Stop => self.container_stop(id).await,
                BatchAction::Restart => self.container_restart(id).await,
                BatchAction::Pause => self.container_pause(id).await,
                BatchAction::Unpause => self.container_unpause(id).await,
                BatchAction::Kill => self.container_kill(id).await,
                BatchAction::Remove => self.container_remove(id, true, false).await,
            };
            out.push((id.clone(), result.err().map(|e| e.message)));
        }
        out
    }

    // images

    pub async fn images(&self, all: bool) -> docker::Result<Vec<Image>> {
        let (_backend, client, key) = self.inventory_session().await;
        let list = self.inventory.images[usize::from(all)]
            .get(key, async {
                Ok(match _backend {
                    #[cfg(target_os = "macos")]
                    Backend::Apple(cli) => apple::images::list(&cli).await?,
                    Backend::EngineApi => images::list(&client, all).await?,
                })
            })
            .await?;
        let ws = self.active_workspace();
        Ok(list
            .into_iter()
            .filter(|i| image_matches_workspace(&i.repo_tags, ws.as_ref()))
            .collect())
    }

    pub async fn image_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        let (_backend, client) = self.session().await;
        match _backend {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::images::inspect(&cli, id).await,
            Backend::EngineApi => images::inspect(&client, id).await,
        }
    }

    pub async fn image_history(&self, id: &str) -> docker::Result<Vec<ImageHistoryEntry>> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("show an image's layer history");
        }
        images::history(&client, id).await
    }

    pub async fn image_search(&self, term: &str) -> docker::Result<Vec<ImageSearchResult>> {
        let (_backend, client) = self.session().await;
        // Apple has no daemon-side search. The Registry view reaches Docker Hub
        // and GitHub over HTTP instead, which works on every backend.
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("search a registry from the daemon");
        }
        images::search(&client, term).await
    }

    /// Search a public registry (Docker Hub, GitHub) for images to pull. Talks
    /// to the registry's web API, so it does not need the engine.
    pub async fn registry_search(
        &self,
        source: RegistrySource,
        query: &str,
    ) -> anyhow::Result<Vec<RegistryResult>> {
        crate::registry::search(source, query).await
    }

    // volumes / networks

    pub async fn volumes(&self) -> docker::Result<Vec<Volume>> {
        let (_backend, client, key) = self.inventory_session().await;
        self.inventory
            .volumes
            .get(key, async {
                #[cfg(target_os = "macos")]
                if let Backend::Apple(cli) = _backend {
                    return apple::volumes::list(&cli).await;
                }
                volumes::list(&client).await
            })
            .await
    }

    pub async fn volume_inspect(&self, name: &str) -> docker::Result<InspectResult> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = _backend {
            return apple::volumes::inspect(&cli, name).await;
        }
        volumes::inspect(&client, name).await
    }

    pub async fn networks(&self) -> docker::Result<Vec<Network>> {
        let (_backend, client, key) = self.inventory_session().await;
        self.inventory
            .networks
            .get(key, async {
                #[cfg(target_os = "macos")]
                if let Backend::Apple(cli) = _backend {
                    return apple::networks::list(&cli).await;
                }
                networks::list(&client).await
            })
            .await
    }

    pub async fn network_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = _backend {
            return apple::networks::inspect(&cli, id).await;
        }
        networks::inspect(&client, id).await
    }

    // streaming

    pub async fn stream_logs<F>(
        &self,
        request_id: &str,
        id: &str,
        opts: &LogOptions,
        on_line: F,
    ) -> docker::Result<()>
    where
        F: FnMut(LogLine) -> bool,
    {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = _backend {
            return apple::containers::stream_logs(
                &cli,
                request_id,
                id,
                opts.tail,
                opts.follow,
                on_line,
            )
            .await;
        }
        logs::stream(&client, request_id, id, opts, on_line).await
    }

    pub async fn stream_stats<F>(&self, id: &str, on_sample: F) -> docker::Result<()>
    where
        F: FnMut(ContainerStats) -> bool,
    {
        let (_backend, client) = self.session().await;
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("stream live resource stats");
        }
        stats::stream(&client, id, on_sample).await
    }

    pub async fn stream_events<F>(&self, on_event: F) -> docker::Result<()>
    where
        F: FnMut(DockerEvent) -> bool,
    {
        self.stream_events_since(None, on_event).await
    }

    pub async fn stream_events_since<F>(
        &self,
        since: Option<i64>,
        mut on_event: F,
    ) -> docker::Result<()>
    where
        F: FnMut(DockerEvent) -> bool,
    {
        let mut paused = self.idle.events.subscribe();
        while *paused.borrow_and_update() {
            if paused.changed().await.is_err() {
                return Ok(());
            }
        }
        let _event = self.idle.event();
        let mut changes = self.selection_changes();
        let (_backend, client) = tokio::select! {
            session = self.session() => session,
            _ = paused.changed() => return Ok(()),
        };
        // Apple publishes no event stream; the UI polls instead. Refusing
        // keeps this from tailing a *different* daemon's events.
        #[cfg(target_os = "macos")]
        if matches!(_backend, Backend::Apple(_)) {
            return refuse("follow the engine's event stream");
        }
        tokio::select! {
            result = system::stream_events_since(&client, since, |event| {
                self.inventory.invalidate();
                on_event(event)
            }) => { self.inventory.invalidate(); result },
            _ = changes.changed() => Ok(()),
            _ = paused.changed() => Ok(()),
        }
    }

    // compose

    /// Compose projects, reconstructed from container labels so stacks show up
    /// with no compose CLI installed.
    pub async fn compose_projects(&self) -> docker::Result<Vec<ComposeProject>> {
        // Read through `containers()` so stacks regroup from whichever engine
        // is active, not always the Engine API one.
        let list = self.containers(true).await?;
        Ok(crate::stacks::group(&list))
    }

    /// Read a compose file and resolve it against the active engine.
    ///
    /// Takes whatever a user picked: the file itself, or a directory Hopper
    /// should find one in. The directory check lives here rather than in the
    /// caller so the UI does not touch the filesystem to decide what to call.
    ///
    /// The plan carries its own warnings, which differ by engine: the same
    /// file planned against Apple's runtime and against Docker produces the
    /// same containers and a different list of what will not be honoured.
    pub fn compose_plan_path(&self, path: &str, opts: &PlanOptions) -> Result<ComposePlan, String> {
        let path = std::path::Path::new(path);
        if path.is_dir() {
            return plan_dir(path, opts, &self.capabilities());
        }
        plan_files(
            &[path.to_string_lossy().to_string()],
            None,
            opts,
            &self.capabilities(),
        )
    }

    /// Re-plan a stack that is already running, from the files its containers
    /// remember.
    ///
    /// Compose writes the file paths onto every container it creates, so a
    /// stack can be brought back up without the user finding the file again.
    pub fn compose_plan_project(
        &self,
        project: &ComposeProject,
        opts: &PlanOptions,
    ) -> Result<ComposePlan, String> {
        if project.config_files.is_empty() {
            return Err(format!(
                "{} does not record which compose file it came from, so it cannot be started again from here.",
                project.name
            ));
        }
        let mut opts = opts.clone();
        opts.project.get_or_insert_with(|| project.name.clone());
        plan_files(&project.config_files, None, &opts, &self.capabilities())
    }

    // settings / workspaces

    pub fn reload_preferences(&self) {
        let _persistence = self.persistence.lock().unwrap();
        let mut settings: Settings =
            store::json::read_or_default(&self.directory.join("settings.json"));
        settings.resources = settings.resources.bounded_to(self.engine_capacity());
        *self.settings.write().unwrap() = settings;
        *self.workspaces.write().unwrap() =
            store::json::read_or_default(&self.directory.join("workspaces.json"));
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn state_directory(&self) -> &std::path::Path {
        &self.directory
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    fn merge_settings(&self, base: &Settings, next: &Settings) -> std::io::Result<Settings> {
        std::fs::create_dir_all(&self.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))?;
        }
        store::transaction::update(&self.directory.join("settings.json"), |current| {
            let mut merged: Settings = store::transaction::merge(base, next, current)?;
            merged.resources = merged.resources.bounded_to(self.engine_capacity());
            let expanded = merged
                .shares
                .iter()
                .any(|grant| !base.shares.contains(grant))
                || merged
                    .shared_paths
                    .iter()
                    .any(|path| !base.shared_paths.contains(path));
            if expanded {
                ::engine::vm::config::shares(&merged).map_err(std::io::Error::other)?;
            }
            ::engine::vm::daemon::document(&merged.daemon).map_err(std::io::Error::other)?;
            Ok(merged)
        })
    }

    pub fn save_settings(&self, next: Settings) -> std::io::Result<()> {
        // Keep the persistence boundary safe as well as startup and engine
        // launch. A future settings editor, imported document, or MCP caller
        // must not be able to write an unexpectedly large managed VM.
        let _persistence = self.persistence.lock().unwrap();
        let base = self.settings();
        let mut next = next;
        next.resources = next.resources.bounded_to(self.engine_capacity());
        // Existing missing folders must not prevent revoking grants or
        // changing unrelated preferences. New or expanded grants still need
        // full validation; startup always validates every remaining grant.
        let expanded = next.shares.iter().any(|grant| !base.shares.contains(grant))
            || next
                .shared_paths
                .iter()
                .any(|path| !base.shared_paths.contains(path));
        if expanded {
            ::engine::vm::config::shares(&next).map_err(std::io::Error::other)?;
        }
        ::engine::vm::daemon::document(&next.daemon).map_err(std::io::Error::other)?;
        let merged = self.merge_settings(&base, &next)?;
        *self.settings.write().unwrap() = merged;
        Ok(())
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        self.workspaces.read().unwrap().clone()
    }

    pub fn active_workspace(&self) -> Option<Workspace> {
        let id = self.settings.read().unwrap().active_workspace.clone()?;
        self.workspaces
            .read()
            .unwrap()
            .iter()
            .find(|w| w.id == id)
            .cloned()
    }

    pub fn set_active_workspace(&self, id: Option<String>) -> std::io::Result<()> {
        let mut settings = self.settings();
        settings.active_workspace = id;
        self.save_settings(settings)
    }

    pub fn save_workspace(&self, ws: Workspace) -> std::io::Result<()> {
        let _persistence = self.persistence.lock().unwrap();
        let base = self.workspaces();
        let next = store::transaction::update(
            &self.directory.join("workspaces.json"),
            |all: Vec<Workspace>| {
                let expected = base.iter().find(|item| item.id == ws.id);
                let current = all.iter().find(|item| item.id == ws.id);
                if current != expected && current != Some(&ws) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "This workspace changed in another process. Reload before saving.",
                    ));
                }
                Ok(store::upsert_workspace(all, ws))
            },
        )?;
        *self.workspaces.write().unwrap() = next;
        Ok(())
    }

    pub fn delete_workspace(&self, id: &str) -> std::io::Result<()> {
        let _persistence = self.persistence.lock().unwrap();
        // Clear selection before removing a record. A failure then leaves a
        // valid unselected workspace, never a dangling reference.
        let base = self.settings();
        if base.active_workspace.as_deref() == Some(id) {
            let mut next = base.clone();
            next.active_workspace = None;
            *self.settings.write().unwrap() = self.merge_settings(&base, &next)?;
        }
        let expected = self.workspaces();
        let next = store::transaction::update(
            &self.directory.join("workspaces.json"),
            |all: Vec<Workspace>| {
                if all.iter().find(|item| item.id == id)
                    != expected.iter().find(|item| item.id == id)
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "This workspace changed in another process. Reload before removing it.",
                    ));
                }
                Ok(store::remove_workspace(all, id))
            },
        )?;
        *self.workspaces.write().unwrap() = next;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Arc<Host> {
        Host::with_directory(
            Client::new(docker::Endpoint::Unix {
                path: "/nonexistent.sock".into(),
            }),
            tempfile::tempdir().unwrap().keep(),
        )
    }

    #[tokio::test]
    #[ignore = "requires HOPPER_TEST_LIFECYCLE=1 and an isolated Hopper VM"]
    async fn startup_admission_allows_stop_to_cancel_its_owner() -> anyhow::Result<()> {
        anyhow::ensure!(
            std::env::var("HOPPER_TEST_LIFECYCLE").as_deref() == Ok("1"),
            "set HOPPER_TEST_LIFECYCLE=1 for an isolated test VM"
        );
        anyhow::ensure!(
            std::env::var("HOPPER_DIR").is_ok_and(|root| !root.trim().is_empty()),
            "set HOPPER_DIR to an isolated state directory"
        );
        let host = Host::from_env();
        let status = host.select_engine().await;
        anyhow::ensure!(
            status.provider == "vm",
            "test must select Hopper's isolated VM"
        );
        if status.connected {
            anyhow::ensure!(
                host.containers(true).await?.is_empty(),
                "test VM must be empty"
            );
        }
        host.stop_engine().await?;
        let starter = Arc::clone(&host);
        let starting = tokio::spawn(async move { starter.start_engine().await });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !host.engines.starting() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let conflict = host.container_start("hopper-admission-check-missing").await;
        anyhow::ensure!(
            conflict.is_err_and(|error| error.is_conflict()),
            "boot admission must exclude resource mutations"
        );
        tokio::time::timeout(Duration::from_secs(15), host.stop_engine()).await??;
        anyhow::ensure!(
            starting.await?.is_err(),
            "cancelled startup must not report success"
        );
        anyhow::ensure!(!host.engines.starting(), "startup flag must clear");
        host.stop_engine().await?;
        let records = host.recent_operations().await?;
        anyhow::ensure!(records.iter().any(
            |record| record.action == "engine_start" && record.state == OperationState::Failed
        ));
        anyhow::ensure!(records
            .last()
            .is_some_and(|record| record.action == "engine_stop"
                && record.state == OperationState::Succeeded));
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires HOPPER_DIR pointing at an isolated, running, empty Hopper VM"]
    async fn idle_suspension_drains_own_events_and_respects_external_clients() -> anyhow::Result<()>
    {
        anyhow::ensure!(
            std::env::var("HOPPER_TEST_IDLE").as_deref() == Ok("1"),
            "set HOPPER_TEST_IDLE=1 for an isolated test VM"
        );
        anyhow::ensure!(
            std::env::var("HOPPER_DIR").is_ok_and(|root| !root.trim().is_empty()),
            "set HOPPER_DIR to an isolated state directory"
        );
        let host = Host::from_env();
        let status = host.select_engine().await;
        anyhow::ensure!(
            status.provider == "vm" && status.connected,
            "test VM must already be running"
        );
        anyhow::ensure!(
            host.containers(true).await?.is_empty(),
            "test VM must be empty"
        );
        let mut settings = host.settings();
        settings.idle_suspend_minutes = Some(1);
        host.save_settings(settings)?;
        let external = host
            .client()
            .send(docker::client::Req::get("/events").no_timeout())
            .await?;
        host.idle.expire();
        anyhow::ensure!(
            !host.suspend_empty_vm().await,
            "external clients must prevent suspension"
        );
        drop(external);
        let watcher = Arc::clone(&host);
        let events = tokio::spawn(async move { watcher.stream_events(|_| true).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while host.idle.active_events.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        host.idle.expire();
        let stopped = host.suspend_empty_vm().await;
        events.abort();
        let _ = events.await;
        anyhow::ensure!(
            stopped,
            "empty VM should stop after draining Hopper's own event connection"
        );
        anyhow::ensure!(
            !host.engine_status().await.connected,
            "VM must actually be stopped"
        );
        Ok(())
    }

    #[test]
    fn separate_hosts_merge_disjoint_preferences_and_reject_conflicting_edits() {
        let directory = tempfile::tempdir().unwrap();
        let client = Client::new(docker::Endpoint::Unix {
            path: "/nonexistent.sock".into(),
        });
        let first = Host::with_directory(client.clone(), directory.path().into());
        let second = Host::with_directory(client, directory.path().into());
        let mut settings = first.settings();
        settings.theme = ThemeMode::Dark;
        first.save_settings(settings).unwrap();
        let mut settings = second.settings();
        settings.autostart_engine = false;
        second.save_settings(settings).unwrap();
        assert_eq!(second.settings().theme, ThemeMode::Dark);
        assert!(!second.settings().autostart_engine);
        let mut settings = second.settings();
        settings.theme = ThemeMode::Light;
        second.save_settings(settings).unwrap();
        let mut settings = first.settings();
        settings.theme = ThemeMode::System;
        assert_eq!(
            first.save_settings(settings).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        first.reload_preferences();
        assert_eq!(first.settings(), second.settings());
        first
            .save_workspace(Workspace {
                id: "shared".into(),
                name: "first".into(),
                ..Default::default()
            })
            .unwrap();
        second.reload_preferences();
        first
            .save_workspace(Workspace {
                id: "shared".into(),
                name: "changed".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(second
            .save_workspace(Workspace {
                id: "shared".into(),
                name: "stale".into(),
                ..Default::default()
            })
            .is_err());
        assert!(second.delete_workspace("shared").is_err());
    }

    #[test]
    fn merged_legacy_and_explicit_grants_are_validated_together() {
        let directory = tempfile::tempdir().unwrap();
        let grant = tempfile::tempdir().unwrap();
        let client = Client::new(docker::Endpoint::Unix {
            path: "/nonexistent.sock".into(),
        });
        let first = Host::with_directory(client.clone(), directory.path().into());
        let second = Host::with_directory(client, directory.path().into());
        let path = grant.path().to_string_lossy().to_string();
        let mut settings = first.settings();
        settings.shared_paths.push(path.clone());
        first.save_settings(settings).unwrap();
        let mut settings = second.settings();
        settings.shares.push(SharedPath {
            path,
            writable: true,
        });
        assert!(second.save_settings(settings).is_err());
        second.reload_preferences();
        assert_eq!(second.settings().shared_paths.len(), 1);
        assert!(second.settings().shares.is_empty());
    }

    #[test]
    fn workspaces_start_empty_and_round_trip_through_the_facade() {
        let h = host();
        let ws = Workspace {
            id: "w1".into(),
            name: "Shop".into(),
            compose_projects: vec!["shop".into()],
            name_pattern: None,
        };
        h.save_workspace(ws.clone()).unwrap();
        assert!(h.workspaces().iter().any(|w| w.id == "w1"));

        h.set_active_workspace(Some("w1".into())).unwrap();
        assert_eq!(h.active_workspace().map(|w| w.name), Some("Shop".into()));
    }

    #[test]
    fn deleting_the_active_workspace_clears_the_selection() {
        let h = host();
        h.save_workspace(Workspace {
            id: "w2".into(),
            name: "Temp".into(),
            ..Default::default()
        })
        .unwrap();
        h.set_active_workspace(Some("w2".into())).unwrap();
        h.delete_workspace("w2").unwrap();

        assert!(
            h.active_workspace().is_none(),
            "a deleted scope must not stay selected"
        );
        assert!(h.settings().active_workspace.is_none());
    }

    #[test]
    fn saving_settings_cannot_persist_an_oversized_managed_engine() {
        let h = host();
        let mut settings = h.settings();
        settings.resources = EngineResources {
            cpus: u32::MAX,
            memory_gib: u32::MAX,
            disk_gib: u32::MAX,
        };
        h.save_settings(settings).unwrap();
        assert_eq!(
            h.settings().resources,
            EngineResources::bounded_to(
                EngineResources {
                    cpus: u32::MAX,
                    memory_gib: u32::MAX,
                    disk_gib: u32::MAX,
                },
                h.engine_capacity()
            )
        );
    }

    #[tokio::test]
    async fn an_unreachable_engine_reports_stopped_rather_than_hanging() {
        let h = host();
        let status = h.engine_status().await;
        assert!(!status.connected);
        assert_eq!(status.state, EngineState::Stopped);
        assert!(status.endpoint.is_some());
    }
}
