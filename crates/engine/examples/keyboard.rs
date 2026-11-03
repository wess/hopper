use anyhow::{ensure, Context};
use engine::machines::{native::sessions::remote, Actor, Machines};
use model::native::{InputDevice, InputEvent};
use std::{io::Write, path::Path, time::Duration};
use tokio::time::Instant;

fn key(code: u16, value: i32) -> Vec<InputEvent> {
  vec![
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
  ]
}

async fn send(control: &mut remote::control::Control, code: u16, value: i32) -> anyhow::Result<()> {
  let start = Instant::now();
  control
    .send(InputDevice::Keyboard, key(code, value))
    .await?;
  eprintln!("key {code} value {value}: {:?}", start.elapsed());
  Ok(())
}

async fn press(control: &mut remote::control::Control, code: u16) -> anyhow::Result<()> {
  send(control, code, 1).await?;
  tokio::time::sleep(Duration::from_millis(50)).await;
  send(control, code, 0).await
}

async fn capture(manager: &Machines, id: &str, path: &Path) -> anyhow::Result<()> {
  let frame = remote::capture(manager, id).await?;
  let file = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(path)?;
  let mut file = std::io::BufWriter::new(file);
  writeln!(file, "P6\n{} {}\n255", frame.width, frame.height)?;
  for pixel in frame.rgba.as_chunks::<4>().0 {
    file.write_all(&pixel[..3])?;
  }
  file.flush()?;
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args = std::env::args_os().skip(1).collect::<Vec<_>>();
  let viewer = args.len() == 2 && args[1] == "--viewer-dummy";
  let reveal = viewer || (args.len() == 2 && args[1] == "--reveal-dummy");
  ensure!(
    args.len() == 1 || reveal,
    "Supply an isolated Windows diagnostic root, optionally followed by --reveal-dummy or --viewer-dummy"
  );
  let root = std::fs::canonicalize(&args[0])?;
  ensure!(
    root
      .file_name()
      .is_some_and(|name| name.to_string_lossy().starts_with("hopperwindows90.")),
    "Use an isolated Windows diagnostic only"
  );
  let manager = Machines {
    root: root.join("machines"),
  };
  let records = std::fs::read_dir(manager.root.join("records"))?
    .take(2)
    .collect::<std::io::Result<Vec<_>>>()?;
  ensure!(records.len() == 1, "Require exactly one diagnostic VM");
  let name = records[0].file_name();
  let id = name
    .to_str()
    .and_then(|name| name.strip_suffix(".json"))
    .context("Invalid record name")?;
  let vm = manager.machine(id, Actor::Agent)?;
  ensure!(
    vm.name == "Isolated Windows installation diagnostic"
      && vm.guest == model::GuestOs::Windows
      && vm.runtime == Some(model::MachineRuntime::Hypervisor),
    "Not an owned native Windows diagnostic"
  );
  ensure!(
    matches!(
      remote::status(&manager, id).await?,
      Some(remote::Status::Running {})
    ),
    "Start the diagnostic VM, focus its empty sign-in field, then close/reopen the viewer to release input"
  );
  let output = tempfile::Builder::new()
    .prefix("hopperkeyboard95.")
    .tempdir_in("/tmp")?
    .keep();
  eprintln!("Inspect dummy input captures in {}", output.display());
  let mut control = remote::control::connect(&manager, id).await?;
  if !viewer {
    // clear earlier dummy input without submitting the sign-in form.
    send(&mut control, 29, 1).await?;
    press(&mut control, 30).await?;
    send(&mut control, 29, 0).await?;
    press(&mut control, 14).await?;
    press(&mut control, 30).await?;
    send(&mut control, 42, 1).await?;
    press(&mut control, 48).await?;
    send(&mut control, 42, 0).await?;
    press(&mut control, 46).await?;
  }
  if reveal {
    let frame = remote::capture(&manager, id).await?;
    ensure!(
      (frame.width, frame.height) == (1024, 768),
      "Unexpected diagnostic display"
    );
    drop(frame);
    // only synthetic dummy input from this diagnostic or its viewer check.
    control
      .send(
        InputDevice::Tablet,
        vec![
          InputEvent {
            kind: 3,
            code: 0,
            value: 608 * 65535 / 1023,
          },
          InputEvent {
            kind: 3,
            code: 1,
            value: 444 * 65535 / 767,
          },
          InputEvent {
            kind: 1,
            code: 272,
            value: 1,
          },
          InputEvent {
            kind: 0,
            code: 0,
            value: 0,
          },
        ],
      )
      .await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    capture(&manager, id, &output.join("revealed.ppm")).await?;
    control
      .send(
        InputDevice::Tablet,
        vec![
          InputEvent {
            kind: 1,
            code: 272,
            value: 0,
          },
          InputEvent {
            kind: 0,
            code: 0,
            value: 0,
          },
        ],
      )
      .await?;
  }
  control.close().await?;
  for (name, delay) in [
    ("immediate.ppm", 100),
    ("one.ppm", 1000),
    ("four.ppm", 3000),
  ] {
    tokio::time::sleep(Duration::from_millis(delay)).await;
    capture(&manager, id, &output.join(name)).await?;
  }
  eprintln!(
    "Compare all three captures for exactly three characters; API acceptance alone is not proof"
  );
  Ok(())
}
