use anyhow::ensure;
use engine::machines::{
  native::{
    assets,
    config::{self, Stage},
    sessions::Sessions,
  },
  windows::deploy,
  Actor, Machines,
};
use model::native::{Command, Result as Reply};
use std::{io::Write, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let assets = assets::locate()?;
  let root = tempfile::tempdir()?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let id = uuid::Uuid::new_v4().to_string();
  let machine = model::Machine {
    id: id.clone(),
    name: "Native hardware startup probe".into(),
    guest: model::GuestOs::Windows,
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 1,
      disk_gib: 64,
    },
    agent_access: true,
    agent_generation: 0,
    runtime: None,
    installer: None,
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &machine,
  )?;
  let paths = config::initialize(&manager, &id)?;
  drop(deploy::create_disk(
    &paths.disk,
    &deploy::Layout {
      disk_gib: 64,
      image_index: 3,
      recovery_mib: 2048,
      recovery_image_bytes: 900 * 1024 * 1024,
    },
  )?);
  let sessions = Sessions::new(manager);
  sessions
    .start_configured(&id, &assets, Stage::System)
    .await?;
  let result = async {
    tokio::time::sleep(Duration::from_secs(5)).await;
    ensure!(
      matches!(
        sessions
          .request(&id, Actor::Person, Command::Status {})
          .await?,
        Reply::Status { paused: false, .. }
      ),
      "Missing native status"
    );
    sessions
      .request(&id, Actor::Person, Command::Pause {})
      .await?;
    let first = sessions.capture(&id, Actor::Agent).await?;
    if let Some(path) = std::env::args().nth(1) {
      let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
      writeln!(file, "P6\n{} {}\n255", first.width, first.height)?;
      for pixel in first.rgba.as_chunks::<4>().0 {
        file.write_all(&pixel[..3])?;
      }
    }
    let second = sessions.capture(&id, Actor::Agent).await?;
    ensure!(first.rgba == second.rgba, "Frame changed while paused");
    sessions
      .request(&id, Actor::Person, Command::Resume {})
      .await?;
    Ok::<_, anyhow::Error>(())
  }
  .await;
  let stop = sessions.stop(&id, Actor::Person).await;
  result?;
  stop?;
  ensure!(
    sessions.state(&id, Actor::Person).await?.is_none(),
    "Session survived stop"
  );
  ensure!(
    paths.variables.join("bank").is_file(),
    "Missing private firmware state"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::MetadataExt;
    ensure!(
      paths.disk.metadata()?.blocks() == 0,
      "Hardware probe wrote the empty target"
    );
  }
  println!("Verified native asset lookup, configured hardware startup, private firmware state, captures and stop. No OS installation was started.");
  Ok(())
}
