use anyhow::{ensure, Context};
use engine::machines::{
  native::{
    assets, config,
    deployment::{Phase, Tools},
    sessions::Sessions,
  },
  windows::setup,
  Actor, Machines,
};
use model::native::Command;
use std::{os::unix::fs::MetadataExt, path::Path};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args: Vec<_> = std::env::args().skip(1).collect();
  ensure!(
    args.len() == 5,
    "Provide installer, drivers root, license, WIM helper and image helper"
  );
  // diagnostic credentials are held only in memory and never touch the OS keychain.
  keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
  let root = tempfile::tempdir()?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let id = uuid::Uuid::new_v4().to_string();
  let record = model::Machine {
    id: id.clone(),
    name: "Prepared hardware probe".into(),
    guest: model::GuestOs::Windows,
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 2,
      disk_gib: 64,
    },
    agent_access: true,
    installer: Some(args[0].clone()),
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &record,
  )?;
  let paths = config::paths(&manager.root, &id)?;
  let assets = assets::locate()?;
  let tools = Tools {
    media: setup::Tools {
      archive: "/usr/bin/tar".into(),
      wim: args[3].clone().into(),
      image: args[4].clone().into(),
    },
    mount: "/usr/sbin/diskutil".into(),
    drivers: args[1].clone().into(),
    license: args[2].clone().into(),
  };
  let sessions = Sessions::new(manager.clone());
  let (progress, mut phase) = watch::channel(Phase::Inspecting);
  let monitor = tokio::spawn(async move {
    while phase.changed().await.is_ok() {
      println!("{:?}", *phase.borrow_and_update());
    }
  });
  let result = async {
    sessions.deploy(&id, &assets, &tools, progress).await?;
    sessions
      .request(&id, Actor::Person, Command::Pause {})
      .await?;
    sessions.stop(&id, Actor::Person).await?;
    ensure!(
      paths.disk.metadata()?.blocks() == 0,
      "Diagnostic guest disk unexpectedly received writes"
    );
    ensure!(
      paths.setup.is_file() && Path::new(&paths.setup.with_extension("json")).is_file(),
      "Missing prepared media"
    );
    ensure!(
      sessions.state(&id, Actor::Person).await?.is_none(),
      "Session remained after stop"
    );
    Ok::<_, anyhow::Error>(())
  }
  .await;
  monitor
    .await
    .context("Preparation progress monitor failed")?;
  if let Err(error) = result {
    let _ = sessions.stop(&id, Actor::Person).await;
    let saved = root.keep();
    anyhow::bail!(
      "Preparation probe failed: {error:#}; private diagnostic state retained at {}",
      saved.display()
    );
  }
  println!("Real media preparation and native hardware startup passed; disk remained unwritten; no OS installation or first boot was verified");
  Ok(())
}
