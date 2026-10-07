use anyhow::{ensure, Context};
use engine::machines::native::{self, Frame, State};
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
  let client = native::launch(
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
  let result = async {
    tokio::time::timeout(Duration::from_secs(90), async {
      loop {
        let Reply::Status { setup, .. } = native::request(&client, Command::Status {}).await?
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
    native::request(&client, Command::Pause {}).await?;
    ensure!(
      native::state(&client) == State::Paused,
      "Missing pause state"
    );
    let first = native::capture(&client).await?;
    let second = native::capture(&client).await?;
    ensure!(
      first.rgba == second.rgba,
      "Guest frame changed while paused"
    );
    native::request(&client, Command::Resume {}).await?;
    ensure!(
      native::state(&client) == State::Running,
      "Missing running state"
    );
    // type a disposable command in the read-only installation environment.
    for code in [
      18, 46, 35, 24, 57, 49, 30, 20, 23, 47, 18, 46, 38, 23, 18, 49, 20, 28,
    ] {
      for value in [1, 0] {
        native::request(
          &client,
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
    native::request(&client, Command::Release {}).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    save(Path::new(&args[6]), &native::capture(&client).await?)?;
    Ok::<_, anyhow::Error>(())
  }
  .await;
  let stopped = native::request(&client, Command::Stop {}).await;
  result?;
  ensure!(
    matches!(stopped?, Reply::Stopped { .. }),
    "Missing stop reply"
  );
  println!(
    "Native client verified setup status, pause, stable captures, resume, guest input and stop"
  );
  Ok(())
}
