use crate::facade::Host;
use crate::runtime::Backend;
use docker::volumes;
use model::*;

impl Host {
    pub async fn volume_create(&self, name: &str) -> docker::Result<Volume> {
        let (operation, guard) = self.admit("volume_create").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::volumes::create(&cli, name).await;
            }
            volumes::create(&client, name, None, &Default::default()).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn volume_remove(&self, name: &str, force: bool) -> docker::Result<()> {
        let (operation, guard) = self.admit("volume_remove").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::volumes::remove(&cli, name).await;
            }
            volumes::remove(&client, name, force).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn volumes_prune(&self) -> docker::Result<PruneReport> {
        let (operation, guard) = self.admit("volumes_prune").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::volumes::prune(&cli).await;
            }
            volumes::prune(&client).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }
}
