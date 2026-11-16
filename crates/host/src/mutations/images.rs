use crate::facade::Host;
#[cfg(target_os = "macos")]
use crate::runtime::Backend;
use docker::images;
use model::*;

impl Host {
    pub async fn image_remove(&self, id: &str, force: bool) -> docker::Result<()> {
        let (operation, guard) = self.admit("image_remove").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::images::remove(&cli, id).await;
            }
            images::remove(&client, id, force).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn image_tag(&self, id: &str, repo: &str, tag: &str) -> docker::Result<()> {
        let (operation, guard) = self.admit("image_tag").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                let target = if tag.is_empty() {
                    repo.to_string()
                } else {
                    format!("{repo}:{tag}")
                };
                return apple::images::tag(&cli, id, &target).await;
            }
            images::tag(&client, id, repo, tag).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn images_prune(&self, all: bool) -> docker::Result<PruneReport> {
        let (operation, guard) = self.admit("images_prune").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                return apple::images::prune(&cli, all).await;
            }
            images::prune(&client, all).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn pull<F>(
        &self,
        request_id: &str,
        reference: &str,
        on: F,
    ) -> docker::Result<images::Transfer>
    where
        F: FnMut(PullProgress),
    {
        let (operation, guard) = self.admit("pull").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                // Apple's CLI reports progress only as terminal control codes, so
                // the transfer is reported as one step rather than faked.
                apple::images::pull(&cli, reference).await?;
                return Ok(images::Transfer {
                    ok: true,
                    error: None,
                });
            }
            images::pull(&client, request_id, reference, on).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }

    pub async fn push<F>(
        &self,
        request_id: &str,
        reference: &str,
        on: F,
    ) -> docker::Result<images::Transfer>
    where
        F: FnMut(PushProgress),
    {
        let (operation, guard) = self.admit("push").await?;
        let result = async {
            let _changed = operation.inventory.mutation();
            let (_backend, client) = operation.session().await;
            #[cfg(target_os = "macos")]
            if let Backend::Apple(cli) = _backend {
                apple::images::push(&cli, reference).await?;
                return Ok(images::Transfer {
                    ok: true,
                    error: None,
                });
            }
            images::push(&client, request_id, reference, on).await
        }
        .await;
        if let Some(guard) = guard {
            guard.finish(result.is_ok());
        }
        result
    }
}
