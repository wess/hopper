use crate::facade::Host;
#[cfg(target_os = "macos")]
use crate::runtime::refuse;
#[cfg(target_os = "macos")]
use crate::runtime::Backend;
use docker::networks;
use model::*;

impl Host {
    pub async fn network_create(&self, input: &NetworkCreateInput) -> docker::Result<String> {
        let (operation, guard) = self.admit("network_create").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::networks::create(&cli, input).await;
            }
            networks::create(&client, input).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    /// Remove a network, refusing Docker's built-ins with a clear reason.
    pub async fn network_remove(&self, id: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("network_remove").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::networks::remove(&cli, id).await;
            }
            if let Ok(list) = networks::list(&client).await {
                if let Some(net) = list.iter().find(|n| n.id == id || n.name == id) {
                    networks::ensure_removable(net)?;
                }
            }
            networks::remove(&client, id).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn network_connect(&self, id: &str, container: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("network_connect").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            // Apple attaches a container to its networks when it is created.
            #[cfg(target_os = "macos")]
            if matches!(_backend, Backend::Apple(_)) {
                return refuse("attach a running container to a network");
            }
            networks::connect(&client, id, container).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn network_disconnect(
        &self,
        id: &str,
        container: &str,
        force: bool,
    ) -> docker::Result<()> {
        let (operation, guard) = self.admit("network_disconnect").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if matches!(_backend, Backend::Apple(_)) {
                return refuse("detach a running container from a network");
            }
            networks::disconnect(&client, id, container, force).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn networks_prune(&self) -> docker::Result<PruneReport> {
        let (operation, guard) = self.admit("networks_prune").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::networks::prune(&cli).await;
            }
            networks::prune(&client).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }
}
