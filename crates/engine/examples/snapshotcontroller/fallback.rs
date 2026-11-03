use anyhow::{ensure, Context};
use engine::machines::{
  native::{
    config,
    restore::{Owner, Phase},
    sessions::{remote, Sessions},
    snapshot::Policy,
    State,
  },
  Actor, Machines,
};
use std::{
  fs,
  os::unix::fs::{FileExt, PermissionsExt},
  time::Duration,
};

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  paths: &config::Paths,
  boot: &model::native::Boot,
  snapshot: &str,
  paused: bool,
) -> anyhow::Result<()> {
  let id = &boot.machine_id;
  if paused {
    sessions
      .request(id, Actor::Person, model::native::Command::Pause {})
      .await?;
  }
  let before = Policy::person(manager.clone(), id)?.inventory()?;
  let path = paths
    .root
    .join("snapshots")
    .join(snapshot.replace('-', ""))
    .join("clock.hrt");
  let file = fs::File::open(&path)?;
  let mut original = [0];
  file.read_exact_at(&mut original, 0)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
  let writer = fs::OpenOptions::new().write(true).open(&path)?;
  fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
  let mut watch = sessions.watch(id).await?;
  ensure!(
    matches!(
      (&*watch.borrow_and_update(), paused),
      (State::Running, false) | (State::Paused, true)
    ),
    "Fallback source lifecycle differs"
  );
  let mutator = writer.try_clone()?;
  let monitor = tokio::spawn(async move {
    tokio::time::timeout(Duration::from_secs(300), async move {
      loop {
        watch
          .changed()
          .await
          .context("Fallback source watch ended")?;
        if matches!(*watch.borrow_and_update(), State::Stopped(_)) {
          mutator.write_all_at(&[original[0] ^ 1], 0)?;
          return Ok::<_, anyhow::Error>(());
        }
      }
    })
    .await?
  });
  let outcome = remote::restoration::restore(
    manager,
    id,
    manager.machine(id, Actor::Agent)?.agent_generation,
    snapshot,
  )
  .await;
  let error = outcome
    .err()
    .context("Fallback probe unexpectedly reported requested restore success")?;
  if !format!("{error:#}").contains("saved prior checkpoint was recovered") {
    monitor.abort();
    anyhow::bail!("Automatic preparation recovery failed: {error:#}");
  }
  monitor.await??;
  let mut damaged = [0];
  file.read_exact_at(&mut damaged, 0)?;
  ensure!(
    damaged != original,
    "Failed target did not remain damaged through recovery"
  );
  ensure!(
    matches!(
      sessions.state(id, Actor::Person).await?,
      Some(State::Running) if !paused
    ) || matches!(sessions.state(id, Actor::Person).await?, Some(State::Paused) if paused),
    "Automatic fallback did not start the saved prior"
  );
  let after = Policy::person(manager.clone(), id)?.inventory()?;
  let added: Vec<_> = after
    .iter()
    .filter(|entry| !before.iter().any(|old| old.id == entry.id))
    .collect();
  ensure!(
    added.len() == 2,
    "Fallback did not preserve prior and independent backup"
  );
  let prior = added
    .iter()
    .find(|entry| entry.name.as_deref() == Some("Before restore"))
    .context("Missing fallback prior")?;
  let backup = added
    .iter()
    .find(|entry| entry.name.as_deref() == Some("Before recovery"))
    .context("Missing fallback backup")?;
  let selected = engine::machines::native::restore::read(
    &fs::File::open(&paths.root)?,
    model::native::machine_identity(id).unwrap(),
  )?
  .context("Missing fallback selection")?;
  ensure!(
    selected.phase == Phase::Active
      && Some(selected.snapshot) == model::native::machine_identity(&prior.id)
      && Some(selected.rollback) == model::native::machine_identity(&backup.id),
    "Fallback selection did not bind prior/backup"
  );
  ensure!(
    Owner::person(manager.clone(), id).is_err(),
    "Running fallback lost runtime ownership"
  );
  writer.write_all_at(&original, 0)?;
  writer.sync_all()?;
  if paused {
    super::paused::unchanged(sessions, manager, boot, &prior.id).await?;
    sessions
      .request(id, Actor::Person, model::native::Command::Resume {})
      .await?;
  }
  println!("Signed HPA1 restore observes source stop, injects late target corruption, recovers the saved prior with an independent backup and the original lifecycle (paused={paused}), preserves the corrupt target through activation and reports the requested restore failure");
  Ok(())
}
