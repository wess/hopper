#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    vz::{
      self,
      mac::{deployment, platform},
      records,
    },
    Actor, Machines,
  };
  use machine::vz::{queue::Check, restore};
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::PermissionsExt,
    sync::{
      atomic::{AtomicUsize, Ordering},
      Arc,
    },
    time::{Duration, Instant},
  };

  let main =
    vz::MainThreadMarker::new().context("Deployment diagnostic requires the main thread")?;
  let run_loop = NSRunLoop::currentRunLoop();
  let discovery = restore::latest();
  let deadline = Instant::now() + Duration::from_secs(120);
  let image = loop {
    if let Some(result) = restore::poll(&discovery)? {
      break result?;
    }
    ensure!(Instant::now() < deadline, "Restore discovery timed out");
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  };
  let root = tempfile::tempdir()?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let machine = records::create(
    &manager,
    model::CreateMachine {
      name: "Deployment diagnostic".into(),
      profile: "macos".into(),
      resources: model::EngineResources {
        cpus: u32::try_from(image.minimum_cpus)?.max(2),
        memory_gib: u32::try_from(image.minimum_memory.div_ceil(1 << 30))?.max(4),
        disk_gib: 64,
      },
      agent_access: true,
      installer: None,
    },
  )?;
  let media = Arc::new(tempfile::tempdir()?);
  let path = media.path().join("restore.ipsw");
  std::fs::write(&path, b"owned malformed media")?;
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
  let check: Check = {
    let manager = manager.clone();
    let id = machine.id.clone();
    Arc::new(move || {
      ensure!(
        manager.machine(&id, Actor::Agent)?.agent_generation == 0,
        "Original installation access changed"
      );
      Ok(())
    })
  };
  let (client, mut owner) = vz::channel();
  let service = vz::Service::new(manager.clone(), client);
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(1)
    .build()?;
  let prepared = platform::prepare(&manager, machine.clone(), image.clone(), check.clone())?;
  prepared.admit(main, &mut owner, media.clone())?;
  let target = manager.root.join("vz").join(&machine.id);
  let original = std::fs::read(target.join("platform"))?;
  let attempt = deployment::begin(&target, &machine.id)?;
  let commits = Arc::new(AtomicUsize::new(0));
  let invoked = commits.clone();
  let commit: Check = Arc::new(move || {
    invoked.fetch_add(1, Ordering::Relaxed);
    deployment::installed(&attempt)
  });
  let installation = owner.install_committed(&machine.id, path.clone(), check.clone(), commit)?;
  ensure!(
    owner.active() && owner.can_retire(&machine.id).is_err(),
    "Installer lost runtime ownership"
  );
  let wake = owner.wake();
  runtime
    .block_on(async { tokio::time::timeout(Duration::from_millis(100), wake.notified()).await })
    .context("Direct installation did not wake the owned runtime")?;
  let deadline = Instant::now() + Duration::from_secs(30);
  while owner.active() {
    owner.tick();
    ensure!(
      Instant::now() < deadline,
      "Malformed installation did not finish"
    );
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  let error = runtime
    .block_on(installation.wait())
    .err()
    .context("Malformed installation succeeded")?;
  ensure!(
    error.to_string().contains("failed"),
    "Unexpected SDK failure: {error}"
  );
  ensure!(
    commits.load(Ordering::Relaxed) == 0,
    "Failed SDK installation committed success"
  );
  ensure!(
    deployment::read(&target, &machine.id)? == Some(deployment::Phase::Installing),
    "Failed installation lost durable recovery intent"
  );
  let receipt = std::fs::read(target.join("deployment"))?;
  owner.retire(&machine.id)?;
  let error = platform::prepare(&manager, machine.clone(), image, check)
    .err()
    .context("Incomplete deployment allowed reinstallation")?;
  ensure!(
    error.to_string().contains("recovery"),
    "Unexpected preparation rejection: {error}"
  );
  let error = runtime
    .block_on(service.prepare_mac_system(&machine.id, Actor::Person))
    .err()
    .context("Incomplete deployment allowed system preparation")?;
  ensure!(
    error.to_string().contains("recovery"),
    "Unexpected system rejection: {error}"
  );
  let rows = runtime.block_on(manager.list_non_windows(Actor::Person))?;
  ensure!(
    rows.len() == 1 && rows[0].state == "Installation recovery required",
    "Incomplete installation was not surfaced"
  );
  ensure!(
    std::fs::read(target.join("platform"))? == original
      && std::fs::read(target.join("deployment"))? == receipt
      && std::fs::read(path)? == b"owned malformed media",
    "Recovery checks changed owned state or source media"
  );
  println!("Signed SDK direct installation woke the runtime, retained durable platform-bound recovery intent, never committed success and rejected reinstallation/system admission while preserving media; no valid IPSW, successful installation, installed system boot or macOS desktop was verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Deployment diagnostic requires Apple silicon macOS")
}
