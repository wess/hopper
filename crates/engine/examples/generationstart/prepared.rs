use anyhow::{ensure, Context};
use engine::machines::{
  native::{config, restore::Owner, sessions::Sessions, State},
  Actor, Machines,
};
use model::native::{Boot, Restore};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};

pub async fn start(
  sessions: &Sessions,
  manager: &Machines,
  root: &Path,
  id: &str,
  worker: &Path,
) -> anyhow::Result<()> {
  let paths = config::paths(&manager.root, id)?;
  std::fs::remove_file(paths.root.join("installation"))?;
  let owner = Owner::agent(manager.clone(), id, 0)?;
  let selected = owner
    .selected()?
    .context("No synthetic active generation")?;
  let digest = format!(
    "{:x}",
    Sha256::digest(uuid::Uuid::parse_str(id)?.as_bytes())
  );
  let boot = Boot {
    machine_id: id.into(),
    firmware: root.join("firmware/windows.fd").to_string_lossy().into(),
    variables: root.join("firmware/variables.fd").to_string_lossy().into(),
    store: paths.variables.to_string_lossy().into(),
    disk: Some(paths.disk.to_string_lossy().into()),
    disk_id: format!("hpr{}", &digest[..17]),
    memory: 1 << 30,
    cpus: 2,
    boot_media: None,
    installer: None,
    timeout_ms: None,
  };
  sessions
    .start_generation(
      id,
      worker,
      boot,
      Restore {
        snapshot_id: uuid::Uuid::from_bytes(selected.snapshot).to_string(),
        generation_id: uuid::Uuid::from_bytes(selected.generation).to_string(),
        policy_generation: 0,
      },
      owner,
    )
    .await
}

pub async fn revoke(
  sessions: &Sessions,
  manager: &Machines,
  id: &str,
  watch: &mut tokio::sync::watch::Receiver<State>,
) -> anyhow::Result<()> {
  for access in [false, true] {
    tokio::time::timeout(Duration::from_secs(5), async {
      loop {
        match manager.set_agent_access(id, access) {
          Ok(_) => return Ok::<_, anyhow::Error>(()),
          Err(error)
            if error
              .chain()
              .filter_map(|error| error.downcast_ref::<std::io::Error>())
              .any(|error| error.kind() == std::io::ErrorKind::WouldBlock) => {}
          Err(error) => return Err(error),
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .context("Policy mutation remained excluded")??;
  }
  tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      if !matches!(*watch.borrow(), State::Running | State::Paused)
        && Owner::person(manager.clone(), id).is_ok()
      {
        return Ok::<_, anyhow::Error>(());
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .context("Revoked native agent worker retained runtime")??;
  tokio::time::sleep(Duration::from_millis(300)).await;
  ensure!(
    !matches!(*watch.borrow(), State::Running | State::Paused),
    "Old agent policy restarted a worker"
  );
  ensure!(
    sessions.capture(id, Actor::Agent).await.is_err(),
    "Revoked session served pixels"
  );
  sessions.stop(id, Actor::Person).await?;
  Ok(())
}
