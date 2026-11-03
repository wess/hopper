#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "generationstart/fixture.rs"]
mod fixture;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "generationstart/prepared.rs"]
mod prepared;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn run() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    native::{assets, config, restore::Owner, sessions::Sessions, State},
    Actor,
  };
  use model::native::StopReason;
  use std::{fs, path::PathBuf, time::Duration};
  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 3,
    "Expected isolated fixture, VM id and signed worker"
  );
  let root = PathBuf::from(&args[0]);
  let id = args[1].to_str().context("Invalid fixture VM id")?;
  let worker = PathBuf::from(&args[2]);
  let manager = fixture::prepare(&root, id, &worker)?;
  let paths = config::paths(&manager.root, id)?;
  let selected = fs::read(paths.root.join("restore.hrr"))?;
  let assets = assets::verify(&worker, &root.join("firmware"))?;
  let sessions = Sessions::new(manager.clone());
  let agent = std::env::var("HOPPER_GENERATION_PREPARED").as_deref() == Ok("1");
  if agent {
    prepared::start(&sessions, &manager, &root, id, &worker).await?;
  } else {
    sessions
      .start_configured(id, &assets, config::Stage::System)
      .await?;
  }
  let mut watch = sessions.watch(id).await?;
  tokio::time::timeout(Duration::from_secs(30), async {
    let mut resets = 0;
    let mut running = false;
    loop {
      match watch.borrow_and_update().clone() {
        State::Running => {
          running = true;
          if resets == 3 {
            break;
          }
        }
        State::Stopped(StopReason::Reset) if running => {
          resets += 1;
          running = false;
        }
        State::Failed(error) => anyhow::bail!("Configured worker failed: {error}"),
        _ => {}
      }
      watch
        .changed()
        .await
        .context("Configured viewer watch closed")?;
    }
    Ok::<_, anyhow::Error>(())
  })
  .await
  .context("Configured guest reset supervision timed out")??;
  if agent {
    prepared::revoke(&sessions, &manager, id, &mut watch).await?;
  } else {
    sessions.stop(id, Actor::Person).await?;
    ensure!(
      matches!(*watch.borrow(), State::Stopped(StopReason::Requested)),
      "Stop lost its viewer state"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    ensure!(
      matches!(*watch.borrow(), State::Stopped(StopReason::Requested)),
      "Requested stop restarted"
    );
  }
  ensure!(
    !paths.disk.exists() && !paths.variables.exists(),
    "Configured boot recreated original resources"
  );
  ensure!(
    fs::read(paths.root.join("restore.hrr"))? == selected,
    "Cold boot rewrote Active selection"
  );
  drop(Owner::person(manager, id)?);
  if agent {
    println!("Signed prepared agent Start runs three real PSCI resets, keeps its viewer watch and Active selection without original disk/EFI or an installation receipt, then off/on revocation reaps the original-policy worker and prevents reauthorization; provenance is synthetic without captured CPU/RAM payloads");
  } else {
    println!("Signed configured Start runs a fresh native guest through three PSCI resets and keeps one viewer watch, preserves Active, never creates original disk/EFI, and stays stopped after requested Stop; provenance is synthetic and has no captured CPU/RAM payloads");
  }
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  return run().await;
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  anyhow::bail!("Configured generation checks require Apple silicon macOS");
}
