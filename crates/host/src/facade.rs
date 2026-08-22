//! `Host` — one method per user-facing operation.
//!
//! Views never touch the Docker client directly; they call through here, which
//! keeps retry, status classification, and workspace scoping in one place.

use docker::client::Client;
use docker::{archive, containers, exec, images, logs, networks, stats, system, volumes};
use model::*;
use std::sync::{Arc, RwLock};

use crate::runtime::{refuse, Backend};

pub struct Host {
    client: Client,
    engines: crate::engine::Engines,
    provider: RwLock<String>,
    managed: RwLock<bool>,
    /// Which client answers. Set when an engine is selected; every operation
    /// resolves its backend from this.
    runtime: RwLock<RuntimeKind>,
    settings: RwLock<Settings>,
    workspaces: RwLock<Vec<Workspace>>,
}

impl Host {
    pub fn new(client: Client) -> Arc<Self> {
        Arc::new(Self {
            engines: crate::engine::Engines::new(client.clone()),
            client,
            provider: RwLock::new("existing".into()),
            managed: RwLock::new(false),
            runtime: RwLock::new(RuntimeKind::default()),
            settings: RwLock::new(store::load_settings()),
            workspaces: RwLock::new(store::load_workspaces()),
        })
    }

    pub fn from_env() -> Arc<Self> {
        Self::new(Client::from_env())
    }

    pub fn client(&self) -> Client {
        self.client.clone()
    }

    /// Point the whole app at a different daemon.
    pub fn set_endpoint(&self, ep: docker::Endpoint, provider: &str, managed: bool) {
        self.client.set_endpoint(ep);
        *self.provider.write().unwrap() = provider.to_string();
        *self.managed.write().unwrap() = managed;
    }

    /// The client that answers for the active engine.
    pub(crate) fn backend(&self) -> Backend {
        Backend::resolve(*self.runtime.read().unwrap())
    }

    /// What the active engine can do, so the UI can hide what it cannot.
    pub fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities::for_runtime(*self.runtime.read().unwrap())
    }

    pub fn runtime_kind(&self) -> RuntimeKind {
        *self.runtime.read().unwrap()
    }

    // --- engine ----------------------------------------------------------

    pub fn engines(&self) -> &crate::engine::Engines {
        &self.engines
    }

    /// Choose an engine and point the client at it.
    pub async fn select_engine(&self) -> EngineStatus {
        let preference = self.settings().engine_preference;
        let status = self.engines.select(preference.as_deref()).await;
        *self.provider.write().unwrap() = status.provider.clone();
        *self.managed.write().unwrap() = status.managed;
        *self.runtime.write().unwrap() = self.engines.registry().active_runtime();
        status
    }

    pub async fn start_engine(&self) -> anyhow::Result<()> {
        let resources = self.settings().resources;
        self.engines.start(resources).await
    }

    pub async fn stop_engine(&self) -> anyhow::Result<()> {
        self.engines.stop().await
    }

    /// Keep forwarded host ports in step with the running containers. Driven
    /// by the Docker event stream, so a container that publishes a port
    /// becomes reachable without the user doing anything.
    pub async fn resync_forwards(&self) -> Vec<String> {
        self.engines.resync_forwards().await
    }

    /// Probe the engine and describe what we found.
    pub async fn engine_status(&self) -> EngineStatus {
        // Apple's runtime answers no socket, so there is nothing to ping —
        // its provider reports on the services instead.
        if self.runtime_kind() == RuntimeKind::Apple {
            return self.engines.status().await;
        }

        let provider = self.provider.read().unwrap().clone();
        let managed = *self.managed.read().unwrap();
        let endpoint = self.client.endpoint().describe();

        if let Err(e) = self.client.ping().await {
            return crate::status::classify(&e, &provider, managed, &endpoint);
        }
        // Negotiate once we know the daemon is up, so an older engine still
        // gets a version we can both speak.
        let _ = self.client.negotiate().await;
        let version = system::version(&self.client)
            .await
            .map(|v| v.version)
            .unwrap_or_default();
        crate::status::connected(&provider, managed, &endpoint, &version)
    }

    pub async fn version(&self) -> docker::Result<SystemVersion> {
        match self.backend() {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::system::version(&cli).await,
            Backend::EngineApi => system::version(&self.client).await,
        }
    }

    pub async fn info(&self) -> docker::Result<SystemInfo> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            // Apple publishes no `info` equivalent, so the dashboard's totals
            // are counted from the lists it does publish.
            let containers = apple::containers::list(&cli, true).await.unwrap_or_default();
            let images = apple::images::list(&cli).await.unwrap_or_default();
            let running = containers
                .iter()
                .filter(|c| c.state == ContainerState::Running)
                .count() as i64;
            let version = apple::system::version(&cli).await.unwrap_or_default();
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
        system::info(&self.client).await
    }

    pub async fn disk_usage(&self) -> docker::Result<DiskUsage> {
        match self.backend() {
            #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::system::df(&cli).await,
            Backend::EngineApi => system::df(&self.client).await,
        }
    }

    pub async fn prune_all(&self) -> Vec<PruneReport> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            // No single prune command, so run the four and keep whichever
            // succeeded — one failing kind must not cost the others.
            let mut out = Vec::new();
            for report in [
                self.containers_prune().await,
                self.images_prune(false).await,
                self.volumes_prune().await,
                self.networks_prune().await,
            ]
            .into_iter()
            .flatten()
            {
                out.push(report);
            }
            return out;
        }
        system::prune_all(&self.client).await
    }

    // --- containers ------------------------------------------------------

    /// List containers, scoped to the active workspace.
    pub async fn containers(&self, all: bool) -> docker::Result<Vec<Container>> {
        let list = match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::list(&cli, all).await?,
            Backend::EngineApi => containers::list(&self.client, all).await?,
        };
        let ws = self.active_workspace();
        Ok(list
            .into_iter()
            .filter(|c| matches_workspace(c, ws.as_ref()))
            .collect())
    }

    pub async fn container_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::inspect(&cli, id).await,
            Backend::EngineApi => containers::inspect(&self.client, id).await,
        }
    }

    pub async fn container_start(&self, id: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::start(&cli, id).await,
            Backend::EngineApi => containers::start(&self.client, id).await,
        }
    }

    pub async fn container_stop(&self, id: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::stop(&cli, id).await,
            Backend::EngineApi => containers::stop(&self.client, id).await,
        }
    }

    pub async fn container_restart(&self, id: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::restart(&cli, id).await,
            Backend::EngineApi => containers::restart(&self.client, id).await,
        }
    }

    pub async fn container_pause(&self, id: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(_) => refuse("pause a container"),
            Backend::EngineApi => containers::pause(&self.client, id).await,
        }
    }

    pub async fn container_unpause(&self, id: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(_) => refuse("resume a paused container"),
            Backend::EngineApi => containers::unpause(&self.client, id).await,
        }
    }

    pub async fn container_kill(&self, id: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::kill(&cli, id).await,
            Backend::EngineApi => containers::kill(&self.client, id).await,
        }
    }

    pub async fn container_rename(&self, id: &str, name: &str) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            // A container's name *is* its id under Apple's runtime, so there
            // is nothing to rename it to.
            Backend::Apple(_) => refuse("rename a container"),
            Backend::EngineApi => containers::rename(&self.client, id, name).await,
        }
    }

    pub async fn container_remove(&self, id: &str, force: bool, volumes: bool) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            // Apple keeps anonymous volumes when a container goes; the
            // volumes view is where they are reclaimed.
            Backend::Apple(cli) => apple::containers::remove(&cli, id, force).await,
            Backend::EngineApi => containers::remove(&self.client, id, force, volumes).await,
        }
    }

    pub async fn container_top(&self, id: &str) -> docker::Result<ProcessList> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(_) => refuse("list the processes in a container"),
            Backend::EngineApi => containers::top(&self.client, id).await,
        }
    }

    pub async fn container_update(&self, id: &str, input: &UpdateInput) -> docker::Result<()> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            // Apple fixes a container's VM size at creation.
            Backend::Apple(_) => refuse("change a container's resources after it is created"),
            Backend::EngineApi => containers::update(&self.client, id, input).await,
        }
    }

    pub async fn container_run(&self, input: &RunInput) -> docker::Result<String> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            // Anything Apple cannot honour is reported rather than dropped.
            for note in apple::containers::unsupported(input) {
                tracing::warn!("{note}");
            }
            return apple::containers::run(&cli, input).await;
        }
        containers::run(&self.client, input).await
    }

    // --- container filesystem (the Files tab) ----------------------------

    /// List a directory inside a container.
    pub async fn container_ls(&self, id: &str, dir: &str) -> docker::Result<Vec<FileEntry>> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("browse a container's filesystem");
        }
        archive::list_dir(&self.client, id, dir).await
    }

    /// Read a file's bytes out of a container.
    pub async fn container_read(&self, id: &str, path: &str) -> docker::Result<Vec<u8>> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("browse a container's filesystem");
        }
        archive::read_file(&self.client, id, path).await
    }

    /// Export a path from a container to a tar file on the host.
    pub async fn container_export(&self, id: &str, path: &str, dest: &std::path::Path) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("export a path from a container");
        }
        archive::export_to(&self.client, id, path, dest).await
    }

    /// Write bytes to a path inside a container.
    pub async fn container_write(&self, id: &str, path: &str, content: &[u8]) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("write into a container");
        }
        archive::write_file(&self.client, id, path, content).await
    }

    // --- interactive exec (the Terminal tab) -----------------------------

    /// Start an interactive shell session in a container.
    pub async fn exec_start<F>(&self, id: &str, shell: Option<&str>, tty: bool, on_output: F) -> docker::Result<exec::Session>
    where
        F: FnMut(String) -> bool + Send + 'static,
    {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("open a shell in a container");
        }
        exec::start(&self.client, id, shell, tty, on_output).await
    }

    pub async fn containers_prune(&self) -> docker::Result<PruneReport> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::containers::prune(&cli).await,
            Backend::EngineApi => containers::prune(&self.client).await,
        }
    }

    /// Apply one action to many containers, reporting per-item failures rather
    /// than aborting the batch. Stopping twelve containers must not stop at
    /// the first one that is already down.
    pub async fn container_batch(
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

    // --- images ----------------------------------------------------------

    pub async fn images(&self, all: bool) -> docker::Result<Vec<Image>> {
        let list = match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::images::list(&cli).await?,
            Backend::EngineApi => images::list(&self.client, all).await?,
        };
        let ws = self.active_workspace();
        Ok(list
            .into_iter()
            .filter(|i| image_matches_workspace(&i.repo_tags, ws.as_ref()))
            .collect())
    }

    pub async fn image_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        match self.backend() {
        #[cfg(target_os = "macos")]
            Backend::Apple(cli) => apple::images::inspect(&cli, id).await,
            Backend::EngineApi => images::inspect(&self.client, id).await,
        }
    }

    pub async fn image_history(&self, id: &str) -> docker::Result<Vec<ImageHistoryEntry>> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("show an image's layer history");
        }
        images::history(&self.client, id).await
    }

    pub async fn image_remove(&self, id: &str, force: bool) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::images::remove(&cli, id).await;
        }
        images::remove(&self.client, id, force).await
    }

    pub async fn image_tag(&self, id: &str, repo: &str, tag: &str) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            let target = if tag.is_empty() { repo.to_string() } else { format!("{repo}:{tag}") };
            return apple::images::tag(&cli, id, &target).await;
        }
        images::tag(&self.client, id, repo, tag).await
    }

    pub async fn images_prune(&self, all: bool) -> docker::Result<PruneReport> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::images::prune(&cli, all).await;
        }
        images::prune(&self.client, all).await
    }

    pub async fn image_search(&self, term: &str) -> docker::Result<Vec<ImageSearchResult>> {
        // Apple has no daemon-side search. The Registry view reaches Docker Hub
        // and GitHub over HTTP instead, which works on every backend.
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("search a registry from the daemon");
        }
        images::search(&self.client, term).await
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

    // --- volumes / networks ----------------------------------------------

    pub async fn volumes(&self) -> docker::Result<Vec<Volume>> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::volumes::list(&cli).await;
        }
        volumes::list(&self.client).await
    }

    pub async fn volume_inspect(&self, name: &str) -> docker::Result<InspectResult> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::volumes::inspect(&cli, name).await;
        }
        volumes::inspect(&self.client, name).await
    }

    pub async fn volume_create(&self, name: &str) -> docker::Result<Volume> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::volumes::create(&cli, name).await;
        }
        volumes::create(&self.client, name, None, &Default::default()).await
    }

    pub async fn volume_remove(&self, name: &str, force: bool) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::volumes::remove(&cli, name).await;
        }
        volumes::remove(&self.client, name, force).await
    }

    pub async fn volumes_prune(&self) -> docker::Result<PruneReport> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::volumes::prune(&cli).await;
        }
        volumes::prune(&self.client).await
    }

    pub async fn networks(&self) -> docker::Result<Vec<Network>> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::networks::list(&cli).await;
        }
        networks::list(&self.client).await
    }

    pub async fn network_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::networks::inspect(&cli, id).await;
        }
        networks::inspect(&self.client, id).await
    }

    pub async fn network_create(&self, input: &NetworkCreateInput) -> docker::Result<String> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::networks::create(&cli, input).await;
        }
        networks::create(&self.client, input).await
    }

    /// Remove a network, refusing Docker's built-ins with a clear reason.
    pub async fn network_remove(&self, id: &str) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::networks::remove(&cli, id).await;
        }
        if let Ok(list) = networks::list(&self.client).await {
            if let Some(net) = list.iter().find(|n| n.id == id || n.name == id) {
                networks::ensure_removable(net)?;
            }
        }
        networks::remove(&self.client, id).await
    }

    pub async fn network_connect(&self, id: &str, container: &str) -> docker::Result<()> {
        // Apple attaches a container to its networks when it is created.
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("attach a running container to a network");
        }
        networks::connect(&self.client, id, container).await
    }

    pub async fn network_disconnect(
        &self,
        id: &str,
        container: &str,
        force: bool,
    ) -> docker::Result<()> {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("detach a running container from a network");
        }
        networks::disconnect(&self.client, id, container, force).await
    }

    pub async fn networks_prune(&self) -> docker::Result<PruneReport> {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            return apple::networks::prune(&cli).await;
        }
        networks::prune(&self.client).await
    }

    // --- streaming -------------------------------------------------------

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
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
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
        logs::stream(&self.client, request_id, id, opts, on_line).await
    }

    pub async fn stream_stats<F>(&self, id: &str, on_sample: F) -> docker::Result<()>
    where
        F: FnMut(ContainerStats) -> bool,
    {
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("stream live resource stats");
        }
        stats::stream(&self.client, id, on_sample).await
    }

    pub async fn stream_events<F>(&self, on_event: F) -> docker::Result<()>
    where
        F: FnMut(DockerEvent) -> bool,
    {
        // Apple publishes no event stream; the UI polls instead. Refusing
        // keeps this from tailing a *different* daemon's events.
        #[cfg(target_os = "macos")]
        if matches!(self.backend(), Backend::Apple(_)) {
            return refuse("follow the engine's event stream");
        }
        system::stream_events(&self.client, on_event).await
    }

    pub async fn pull<F>(&self, request_id: &str, reference: &str, on: F) -> docker::Result<images::Transfer>
    where
        F: FnMut(PullProgress),
    {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            // Apple's CLI reports progress only as terminal control codes, so
            // the transfer is reported as one step rather than faked.
            apple::images::pull(&cli, reference).await?;
            return Ok(images::Transfer { ok: true, error: None });
        }
        images::pull(&self.client, request_id, reference, on).await
    }

    pub async fn push<F>(&self, request_id: &str, reference: &str, on: F) -> docker::Result<images::Transfer>
    where
        F: FnMut(PushProgress),
    {
        #[cfg(target_os = "macos")]
        if let Backend::Apple(cli) = self.backend() {
            apple::images::push(&cli, reference).await?;
            return Ok(images::Transfer { ok: true, error: None });
        }
        images::push(&self.client, request_id, reference, on).await
    }

    // --- compose ---------------------------------------------------------

    /// Compose projects, reconstructed from container labels so stacks show up
    /// with no compose CLI installed.
    pub async fn compose_projects(&self) -> docker::Result<Vec<ComposeProject>> {
        // Read through `containers()` so stacks regroup from whichever engine
        // is active, not always the Engine API one.
        let list = self.containers(true).await?;
        Ok(crate::compose::group(&list))
    }

    // --- settings / workspaces -------------------------------------------

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn save_settings(&self, next: Settings) {
        if let Err(e) = store::save_settings(&next) {
            tracing::warn!("could not persist settings: {e}");
        }
        *self.settings.write().unwrap() = next;
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

    pub fn set_active_workspace(&self, id: Option<String>) {
        let mut settings = self.settings.write().unwrap();
        settings.active_workspace = id;
        let _ = store::save_settings(&settings);
    }

    pub fn save_workspace(&self, ws: Workspace) {
        let mut all = self.workspaces.write().unwrap();
        *all = store::upsert_workspace(all.clone(), ws);
        let _ = store::save_workspaces(&all);
    }

    pub fn delete_workspace(&self, id: &str) {
        let mut all = self.workspaces.write().unwrap();
        *all = store::remove_workspace(all.clone(), id);
        let _ = store::save_workspaces(&all);
        drop(all);
        // A deleted workspace must not stay selected, or every view filters to
        // a scope that no longer exists.
        if self.settings.read().unwrap().active_workspace.as_deref() == Some(id) {
            self.set_active_workspace(None);
        }
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
        Host::new(Client::new(docker::Endpoint::Unix {
            path: "/nonexistent.sock".into(),
        }))
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
        h.save_workspace(ws.clone());
        assert!(h.workspaces().iter().any(|w| w.id == "w1"));

        h.set_active_workspace(Some("w1".into()));
        assert_eq!(h.active_workspace().map(|w| w.name), Some("Shop".into()));
    }

    #[test]
    fn deleting_the_active_workspace_clears_the_selection() {
        let h = host();
        h.save_workspace(Workspace {
            id: "w2".into(),
            name: "Temp".into(),
            ..Default::default()
        });
        h.set_active_workspace(Some("w2".into()));
        h.delete_workspace("w2");

        assert!(
            h.active_workspace().is_none(),
            "a deleted scope must not stay selected"
        );
        assert!(h.settings().active_workspace.is_none());
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
