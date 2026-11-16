use crate::facade::Host;
#[cfg(target_os = "macos")]
use crate::runtime::refuse;
use crate::runtime::Backend;
use docker::archive;
use docker::containers;
use model::*;

impl Host {
    pub async fn container_start(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_start").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(cli) => apple::containers::start(&cli, id).await,
                Backend::EngineApi => containers::start(&client, id).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_stop(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_stop").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(cli) => apple::containers::stop(&cli, id).await,
                Backend::EngineApi => containers::stop(&client, id).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_restart(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_restart").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(cli) => apple::containers::restart(&cli, id).await,
                Backend::EngineApi => containers::restart(&client, id).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_pause(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_pause").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(_) => refuse("pause a container"),
                Backend::EngineApi => containers::pause(&client, id).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_unpause(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_unpause").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(_) => refuse("resume a paused container"),
                Backend::EngineApi => containers::unpause(&client, id).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_kill(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_kill").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(cli) => apple::containers::kill(&cli, id).await,
                Backend::EngineApi => containers::kill(&client, id).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_rename(&self, id: &str, name: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_rename").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                // A container's name *is* its id under Apple's runtime, so there
                // is nothing to rename it to.
                Backend::Apple(_) => refuse("rename a container"),
                Backend::EngineApi => containers::rename(&client, id, name).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_remove(
        &self,
        id: &str,
        force: bool,
        volumes: bool,
    ) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_remove").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                // Apple keeps anonymous volumes when a container goes; the
                // volumes view is where they are reclaimed.
                Backend::Apple(cli) => apple::containers::remove(&cli, id, force).await,
                Backend::EngineApi => containers::remove(&client, id, force, volumes).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_update(&self, id: &str, input: &UpdateInput) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_update").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                // Apple fixes a container's VM size at creation.
                Backend::Apple(_) => refuse("change a container's resources after it is created"),
                Backend::EngineApi => containers::update(&client, id, input).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn container_run(&self, input: &RunInput) -> docker::Result<String> {
        let (operation, guard) = self.admit("container_run").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                // Anything Apple cannot honour is reported rather than dropped.
                for note in apple::containers::unsupported(input) {
                    tracing::warn!("{note}");
                }
                return apple::containers::run(&cli, input).await;
            }
            containers::run(&client, input).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    /// Write bytes to a path inside a container.
    pub async fn container_write(
        &self,
        id: &str,
        path: &str,
        content: &[u8],
    ) -> docker::Result<()> {
        let (operation, guard) = self.admit("container_write").await?;
        let result = async {
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if matches!(_backend, Backend::Apple(_)) {
                return refuse("write into a container");
            }
            archive::write_file(&client, id, path, content).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn containers_prune(&self) -> docker::Result<PruneReport> {
        let (operation, guard) = self.admit("containers_prune").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            match _backend {
                #[cfg(target_os = "macos")]
                Backend::Apple(cli) => apple::containers::prune(&cli).await,
                Backend::EngineApi => containers::prune(&client).await,
            }
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }
}
