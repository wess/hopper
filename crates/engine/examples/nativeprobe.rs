use anyhow::{ensure, Context};
use engine::machines::{
  native::{sessions::Sessions, Frame, State},
  Actor, Machines,
};
use model::native::{Boot, Command, InputDevice, InputEvent, Result as Reply, SetupStatus};
use std::{io::Write, path::Path, time::Duration};

fn save(path: &Path, frame: &Frame) -> anyhow::Result<()> {
  let mut file = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(path)?;
  writeln!(file, "P6\n{} {}\n255", frame.width, frame.height)?;
  let pixels: Vec<_> = frame
    .rgba
    .as_chunks::<4>()
    .0
    .iter()
    .flat_map(|pixel| pixel[..3].iter().copied())
    .collect();
  file.write_all(&pixels)?;
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args: Vec<_> = std::env::args().skip(1).collect();
  ensure!(args.len() == 7, "Provide worker, firmware, variables template, private state directory, boot DVD, installer DVD and a new output PPM");
  let directory = tempfile::tempdir()?;
  let manager = Machines {
    root: directory.path().join("machines"),
  };
  let id = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let record = model::Machine {
    id: id.into(),
    name: "Native session probe".into(),
    guest: model::GuestOs::Windows,
    profile: "windows".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 2,
      disk_gib: 64,
    },
    agent_access: true,
    installer: None,
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &record,
  )?;
  let sessions = Sessions::new(manager.clone());
  sessions
    .start(
      id,
      Path::new(&args[0]),
      Boot {
        firmware: args[1].clone(),
        variables: args[2].clone(),
        store: args[3].clone(),
        boot_media: Some(args[4].clone()),
        installer: Some(args[5].clone()),
        disk: None,
        disk_id: "hoppernativeprobe001".into(),
        memory: 2 * 1024 * 1024 * 1024,
        cpus: 2,
        timeout_ms: Some(120_000),
      },
    )
    .await?;
  drop(sessions.watch(id).await?);
  let result = async {
    manager.set_agent_access(id, false)?;
    ensure!(
      sessions.capture(id, Actor::Agent).await.is_err(),
      "Revoked agent capture was accepted"
    );
    manager.set_agent_access(id, true)?;
    tokio::time::timeout(Duration::from_secs(90), async {
      loop {
        let Reply::Status { setup, .. } = sessions
          .request(id, Actor::Person, Command::Status {})
          .await?
        else {
          anyhow::bail!("Worker stopped before setup was observable");
        };
        match setup {
          SetupStatus::Failed { phase } => {
            ensure!(
              phase == model::native::SetupPhase::Partition,
              "Unexpected setup failure: {phase:?}"
            );
            break;
          }
          SetupStatus::Invalid {} | SetupStatus::Deployed {} => {
            anyhow::bail!("Unexpected deployment status")
          }
          _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
      }
      Ok::<_, anyhow::Error>(())
    })
    .await
    .context("WinPE setup readiness timed out")??;
    sessions
      .request(id, Actor::Person, Command::Pause {})
      .await?;
    ensure!(
      sessions.state(id, Actor::Person).await? == Some(State::Paused),
      "Missing pause state"
    );
    let first = sessions.capture(id, Actor::Agent).await?;
    let second = sessions.capture(id, Actor::Agent).await?;
    ensure!(
      first.rgba == second.rgba,
      "Guest frame changed while paused"
    );
    sessions
      .request(id, Actor::Person, Command::Resume {})
      .await?;
    ensure!(
      sessions.state(id, Actor::Person).await? == Some(State::Running),
      "Missing running state"
    );
    // type a disposable command in the read-only installation environment.
    for code in [
      18, 46, 35, 24, 57, 49, 30, 20, 23, 47, 18, 46, 38, 23, 18, 49, 20, 28,
    ] {
      for value in [1, 0] {
        sessions
          .request(
            id,
            Actor::Person,
            Command::Input {
              device: InputDevice::Keyboard,
              events: vec![
                InputEvent {
                  kind: 1,
                  code,
                  value,
                },
                InputEvent {
                  kind: 0,
                  code: 0,
                  value: 0,
                },
              ],
            },
          )
          .await?;
      }
    }
    sessions
      .request(id, Actor::Person, Command::Release {})
      .await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    save(
      Path::new(&args[6]),
      &sessions.capture(id, Actor::Agent).await?,
    )?;
    Ok::<_, anyhow::Error>(())
  }
  .await;
  let stopped = sessions.stop(id, Actor::Person).await;
  result?;
  stopped?;
  ensure!(
    sessions.state(id, Actor::Person).await?.is_none(),
    "Session remained after stop"
  );
  println!(
    "Native sessions verified policy, viewer independence, setup status, pause, captures, input and stop"
  );
  Ok(())
}
