use anyhow::{ensure, Context};
use engine::machines::{
  native::{deployment::Phase, sessions::Sessions, Frame, State},
  Actor, Machines,
};
use model::native::{Installation, SetupStatus};
use std::{io::Write, path::Path, time::Duration};
use tokio::{sync::watch, time::Instant};

fn capture(path: &Path, frame: &Frame) -> anyhow::Result<()> {
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
  ensure!(
    args.is_empty() || (args.len() == 2 && (args[0] == "--resume" || args[0] == "--cache")),
    "Use no arguments for a new diagnostic, --cache with a prior diagnostic, or --resume with its retained root"
  );
  keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
  let fresh = args.is_empty() || args[0] == "--cache";
  let root = if fresh {
    tempfile::Builder::new()
      .prefix("hopperwindows90.")
      .tempdir_in("/tmp")?
      .keep()
  } else {
    let path = std::fs::canonicalize(&args[1])?;
    ensure!(
      path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("hopperwindows90.")),
      "Resume only an isolated Windows diagnostic"
    );
    path
  };
  eprintln!("Isolated Windows evidence and media: {}", root.display());
  if args.first().is_some_and(|arg| arg == "--cache") {
    let prior = std::fs::canonicalize(&args[1])?;
    ensure!(
      prior
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("hopperwindows90.")),
      "Reuse only an isolated Windows diagnostic cache"
    );
    let relative = "machines/native/images/windows/installer";
    let cache = root.join(relative);
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
      .recursive(true)
      .mode(0o700)
      .create(&cache)?;
    for name in ["installer.iso", "catalogue.cab", "manifest.json"] {
      let source = prior.join(relative).join(name);
      ensure!(
        std::fs::symlink_metadata(&source)?.is_file(),
        "Cache source must be regular"
      );
      ensure!(
        std::process::Command::new("/bin/cp")
          .arg("-c")
          .arg(&source)
          .arg(cache.join(name))
          .status()?
          .success(),
        "Clone isolated diagnostic cache"
      );
    }
    eprintln!("Cloned prior media; production acquisition will verify it before reuse");
  }
  let output = root.join(format!("run{}", uuid::Uuid::new_v4().simple()));
  std::fs::create_dir(&output)?;
  eprintln!("Guest display evidence: {}", output.display());
  let manager = Machines {
    root: root.join("machines"),
  };
  let sessions = Sessions::new(manager.clone());
  let record = if fresh {
    sessions.create_windows(model::CreateMachine {
      name: "Isolated Windows installation diagnostic".into(),
      profile: "windows".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 4,
        disk_gib: 64,
      },
      installer: None,
      agent_access: true,
    })?
  } else {
    let records = std::fs::read_dir(manager.root.join("records"))?
      .take(2)
      .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(
      records.len() == 1,
      "Diagnostic must have exactly one record"
    );
    let name = records[0].file_name();
    let id = name
      .to_str()
      .and_then(|name| name.strip_suffix(".json"))
      .context("Invalid diagnostic record filename")?;
    let record = manager.machine(id, Actor::Person)?;
    ensure!(
      record.name == "Isolated Windows installation diagnostic"
        && record.guest == model::GuestOs::Windows
        && record.runtime == Some(model::MachineRuntime::Hypervisor),
      "Not an owned Windows diagnostic record"
    );
    ensure!(
      matches!(
        sessions.installation(&record.id, Actor::Person)?,
        Some(Installation::SystemStarted {} | Installation::HandoffFailed {})
      ),
      "Resume requires system-boot intent; preserve incomplete deployments for recovery"
    );
    record
  };
  let id = &record.id;
  eprintln!("Diagnostic VM: {id}");
  let (progress, phase) = watch::channel(Phase::Inspecting);
  let monitor_root = manager.root.clone();
  let monitor_id = id.clone();
  let monitor = tokio::spawn(async move {
    let mut ticks = tokio::time::interval(Duration::from_secs(15));
    loop {
      ticks.tick().await;
      eprintln!("Windows preparation: {:?}", *phase.borrow());
      let path = monitor_root
        .join("native")
        .join(&monitor_id)
        .join("download");
      if let Ok(info) = tokio::fs::metadata(&path).await {
        if info.is_file() && info.len() <= 4096 {
          if let Ok(message) = tokio::fs::read_to_string(path).await {
            eprintln!("{message}");
          }
        }
      }
    }
  });
  let started = tokio::time::timeout(
    Duration::from_secs(45 * 60),
    sessions.start_windows(id, progress),
  )
  .await;
  monitor.abort();
  let result = async {
    started.context("Windows preparation timed out")??;
    let deadline = Instant::now() + Duration::from_secs(60 * 60);
    let mut previous = None;
    let mut hardware = None;
    let mut system_at = None;
    let mut capture_at = Instant::now();
    let mut frame = 0;
    loop {
      ensure!(Instant::now() < deadline, "Windows installation timed out");
      let installation = sessions.installation(id, Actor::Person)?;
      if installation != previous {
        eprintln!("Windows installation: {installation:?}");
        previous = installation.clone();
      }
      ensure!(!matches!(installation,
        Some(Installation::Interrupted { .. } | Installation::HandoffFailed {} |
          Installation::Setup { status: SetupStatus::Failed { .. } | SetupStatus::Invalid {} })),
        "Windows installation failed: {installation:?}");
      let state = sessions.state(id, Actor::Person).await?;
      if state != hardware {
        eprintln!("Windows hardware: {state:?}");
        hardware = state.clone();
      }
      if let Some(State::Failed(message)) = &state { anyhow::bail!("Windows worker failed: {message}"); }
      if let Some(State::Stopped(reason)) = &state {
        let installer_shutdown = matches!(reason, model::native::StopReason::Shutdown)
          && matches!(installation, Some(Installation::Setup { status: SetupStatus::Deployed {} } | Installation::Deployed {} | Installation::Booting {}));
        ensure!(matches!(reason, model::native::StopReason::Reset) || installer_shutdown, "Windows hardware stopped: {reason:?}");
      }
      if matches!(installation, Some(Installation::SystemStarted {})) && system_at.is_none() {
        system_at = Some(Instant::now());
        capture_at = Instant::now();
      }
      if Instant::now() >= capture_at && frame < 300 {
        if let Ok(image) = sessions.capture(id, Actor::Person).await {
          capture(&output.join(format!("frame{frame:03}.ppm")), &image)?;
          frame += 1;
        }
        capture_at = Instant::now() + Duration::from_secs(if system_at.is_some() { 2 } else { 30 });
      }
      if matches!(installation, Some(Installation::SystemStarted {})) {
        let since = system_at.get_or_insert_with(Instant::now);
        if since.elapsed() >= Duration::from_secs(5 * 60) {
          capture(&output.join("system.ppm"), &sessions.capture(id, Actor::Person).await?)?;
          println!("Windows system hardware ran for five minutes; inspect captured frames for actual desktop readiness");
          return Ok::<_, anyhow::Error>(());
        }
      }
      tokio::time::sleep(Duration::from_millis(500)).await;
    }
  }.await;
  let stopped = sessions.stop(id, Actor::Person).await;
  result?;
  stopped?;
  Ok(())
}
