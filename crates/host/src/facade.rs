//! `Host` — one method per user-facing operation.
//!
//! Views never touch the Docker client directly; they call through here, which
//! keeps retry, status classification, and workspace scoping in one place.

use docker::client::Client;
use docker::{archive, containers, exec, images, logs, networks, stats, system, volumes};
use model::*;
use std::sync::{Arc, RwLock};

pub struct Host {
    client: Client,
    engines: crate::engine::Engines,
    provider: RwLock<String>,
    managed: RwLock<bool>,
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
        system::version(&self.client).await
    }

    pub async fn info(&self) -> docker::Result<SystemInfo> {
        system::info(&self.client).await
    }

    pub async fn disk_usage(&self) -> docker::Result<DiskUsage> {
        system::df(&self.client).await
    }

    pub async fn prune_all(&self) -> Vec<PruneReport> {
        system::prune_all(&self.client).await
    }

    // --- containers ------------------------------------------------------

    /// List containers, scoped to the active workspace.
    pub async fn containers(&self, all: bool) -> docker::Result<Vec<Container>> {
        let list = containers::list(&self.client, all).await?;
        let ws = self.active_workspace();
        Ok(list
            .into_iter()
            .filter(|c| matches_workspace(c, ws.as_ref()))
            .collect())
    }

    pub async fn container_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        containers::inspect(&self.client, id).await
    }

    pub async fn container_start(&self, id: &str) -> docker::Result<()> {
        containers::start(&self.client, id).await
    }

    pub async fn container_stop(&self, id: &str) -> docker::Result<()> {
        containers::stop(&self.client, id).await
    }

    pub async fn container_restart(&self, id: &str) -> docker::Result<()> {
        containers::restart(&self.client, id).await
    }

    pub async fn container_pause(&self, id: &str) -> docker::Result<()> {
        containers::pause(&self.client, id).await
    }

    pub async fn container_unpause(&self, id: &str) -> docker::Result<()> {
        containers::unpause(&self.client, id).await
    }

    pub async fn container_kill(&self, id: &str) -> docker::Result<()> {
        containers::kill(&self.client, id).await
    }

    pub async fn container_rename(&self, id: &str, name: &str) -> docker::Result<()> {
        containers::rename(&self.client, id, name).await
    }

    pub async fn container_remove(&self, id: &str, force: bool, volumes: bool) -> docker::Result<()> {
        containers::remove(&self.client, id, force, volumes).await
    }

    pub async fn container_top(&self, id: &str) -> docker::Result<ProcessList> {
        containers::top(&self.client, id).await
    }

    pub async fn container_update(&self, id: &str, input: &UpdateInput) -> docker::Result<()> {
        containers::update(&self.client, id, input).await
    }

    pub async fn container_run(&self, input: &RunInput) -> docker::Result<String> {
        containers::run(&self.client, input).await
    }

    // --- container filesystem (the Files tab) ----------------------------

    /// List a directory inside a container.
    pub async fn container_ls(&self, id: &str, dir: &str) -> docker::Result<Vec<FileEntry>> {
        archive::list_dir(&self.client, id, dir).await
    }

    /// Read a file's bytes out of a container.
    pub async fn container_read(&self, id: &str, path: &str) -> docker::Result<Vec<u8>> {
        archive::read_file(&self.client, id, path).await
    }

    /// Export a path from a container to a tar file on the host.
    pub async fn container_export(&self, id: &str, path: &str, dest: &std::path::Path) -> docker::Result<()> {
        archive::export_to(&self.client, id, path, dest).await
    }

    /// Write bytes to a path inside a container.
    pub async fn container_write(&self, id: &str, path: &str, content: &[u8]) -> docker::Result<()> {
        archive::write_file(&self.client, id, path, content).await
    }

    // --- interactive exec (the Terminal tab) -----------------------------

    /// Start an interactive shell session in a container.
    pub async fn exec_start<F>(&self, id: &str, shell: Option<&str>, tty: bool, on_output: F) -> docker::Result<exec::Session>
    where
        F: FnMut(String) -> bool + Send + 'static,
    {
        exec::start(&self.client, id, shell, tty, on_output).await
    }

    pub async fn containers_prune(&self) -> docker::Result<PruneReport> {
        containers::prune(&self.client).await
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
        let list = images::list(&self.client, all).await?;
        let ws = self.active_workspace();
        Ok(list
            .into_iter()
            .filter(|i| image_matches_workspace(&i.repo_tags, ws.as_ref()))
            .collect())
    }

    pub async fn image_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        images::inspect(&self.client, id).await
    }

    pub async fn image_history(&self, id: &str) -> docker::Result<Vec<ImageHistoryEntry>> {
        images::history(&self.client, id).await
    }

    pub async fn image_remove(&self, id: &str, force: bool) -> docker::Result<()> {
        images::remove(&self.client, id, force).await
    }

    pub async fn image_tag(&self, id: &str, repo: &str, tag: &str) -> docker::Result<()> {
        images::tag(&self.client, id, repo, tag).await
    }

    pub async fn images_prune(&self, all: bool) -> docker::Result<PruneReport> {
        images::prune(&self.client, all).await
    }

    pub async fn image_search(&self, term: &str) -> docker::Result<Vec<ImageSearchResult>> {
        images::search(&self.client, term).await
    }

    // --- volumes / networks ----------------------------------------------

    pub async fn volumes(&self) -> docker::Result<Vec<Volume>> {
        volumes::list(&self.client).await
    }

    pub async fn volume_inspect(&self, name: &str) -> docker::Result<InspectResult> {
        volumes::inspect(&self.client, name).await
    }

    pub async fn volume_create(&self, name: &str) -> docker::Result<Volume> {
        volumes::create(&self.client, name, None, &Default::default()).await
    }

    pub async fn volume_remove(&self, name: &str, force: bool) -> docker::Result<()> {
        volumes::remove(&self.client, name, force).await
    }

    pub async fn volumes_prune(&self) -> docker::Result<PruneReport> {
        volumes::prune(&self.client).await
    }

    pub async fn networks(&self) -> docker::Result<Vec<Network>> {
        networks::list(&self.client).await
    }

    pub async fn network_inspect(&self, id: &str) -> docker::Result<InspectResult> {
        networks::inspect(&self.client, id).await
    }

    pub async fn network_create(&self, input: &NetworkCreateInput) -> docker::Result<String> {
        networks::create(&self.client, input).await
    }

    /// Remove a network, refusing Docker's built-ins with a clear reason.
    pub async fn network_remove(&self, id: &str) -> docker::Result<()> {
        if let Ok(list) = networks::list(&self.client).await {
            if let Some(net) = list.iter().find(|n| n.id == id || n.name == id) {
                networks::ensure_removable(net)?;
            }
        }
        networks::remove(&self.client, id).await
    }

    pub async fn network_connect(&self, id: &str, container: &str) -> docker::Result<()> {
        networks::connect(&self.client, id, container).await
    }

    pub async fn network_disconnect(
        &self,
        id: &str,
        container: &str,
        force: bool,
    ) -> docker::Result<()> {
        networks::disconnect(&self.client, id, container, force).await
    }

    pub async fn networks_prune(&self) -> docker::Result<PruneReport> {
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
        logs::stream(&self.client, request_id, id, opts, on_line).await
    }

    pub async fn stream_stats<F>(&self, id: &str, on_sample: F) -> docker::Result<()>
    where
        F: FnMut(ContainerStats) -> bool,
    {
        stats::stream(&self.client, id, on_sample).await
    }

    pub async fn stream_events<F>(&self, on_event: F) -> docker::Result<()>
    where
        F: FnMut(DockerEvent) -> bool,
    {
        system::stream_events(&self.client, on_event).await
    }

    pub async fn pull<F>(&self, request_id: &str, reference: &str, on: F) -> docker::Result<images::Transfer>
    where
        F: FnMut(PullProgress),
    {
        images::pull(&self.client, request_id, reference, on).await
    }

    pub async fn push<F>(&self, request_id: &str, reference: &str, on: F) -> docker::Result<images::Transfer>
    where
        F: FnMut(PushProgress),
    {
        images::push(&self.client, request_id, reference, on).await
    }

    // --- compose ---------------------------------------------------------

    /// Compose projects, reconstructed from container labels so stacks show up
    /// with no compose CLI installed.
    pub async fn compose_projects(&self) -> docker::Result<Vec<ComposeProject>> {
        let list = containers::list(&self.client, true).await?;
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
