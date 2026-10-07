use super::{
  super::{assets::Assets, config, deployment::Phase, installation},
  owner, Slot,
};
use crate::machines::Machines;
use anyhow::ensure;
use model::native::Installation;
use tokio::sync::watch;

// caller owns the VM operation lease and slot through old-worker reap and publication.
pub(super) async fn start(
  manager: &Machines,
  id: &str,
  assets: &Assets,
  progress: &watch::Sender<Phase>,
  slot: &Slot,
  session: &mut Option<owner::Session>,
) -> anyhow::Result<u64> {
  ensure!(session.is_none(), "Previous native worker is still owned");
  installation::save(manager, id, Installation::Booting {})?;
  progress.send_replace(Phase::SystemBoot);
  let boot = config::prepare(manager, id, assets, config::Stage::System)?;
  ensure!(
    boot.boot_media.is_none() && boot.installer.is_none(),
    "System boot retained installation media"
  );
  let runtime = manager.guard(id, ".runtime")?;
  let client = super::super::launch(&assets.worker, boot).await?;
  let created = owner::started(
    manager,
    id,
    client,
    runtime,
    Some(Installation::SystemStarted {}),
  )
  .await?;
  let epoch = owner::publish(slot, &created.client);
  *session = Some(created);
  Ok(epoch)
}
