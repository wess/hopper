#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    vz::{self, mac::platform, records, Service, State},
    Actor, Machines,
  };
  use machine::vz::{queue::Check, restore};
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    sync::Arc,
    time::{Duration, Instant},
  };

  let main =
    vz::MainThreadMarker::new().context("Native platform diagnostic requires the main thread")?;
  let run_loop = NSRunLoop::currentRunLoop();
  let discovery = restore::latest();
  let deadline = Instant::now() + Duration::from_secs(120);
  let image = loop {
    if let Some(result) = restore::poll(&discovery)? {
      break result?;
    }
    ensure!(
      Instant::now() < deadline,
      "Restore metadata discovery timed out"
    );
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
      name: "Native platform diagnostic".into(),
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
  vz::network::set_connected(&manager, &machine.id, false)?;
  let shared = root.path().join("shared");
  std::fs::create_dir(&shared)?;
  vz::sharing::add(
    &manager,
    &machine.id,
    model::MachineFolder {
      name: "work".into(),
      path: shared.to_str().context("Folder path must be UTF-8")?.into(),
      read_only: true,
    },
  )?;
  let (client, mut owner) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(1)
    .build()?;
  let check: Check = {
    let manager = manager.clone();
    let id = machine.id.clone();
    Arc::new(move || {
      let current = manager.machine(&id, Actor::Agent)?;
      ensure!(
        current.agent_generation == 0,
        "Original agent authorization changed"
      );
      Ok(())
    })
  };
  let mut prepared = platform::prepare(&manager, machine.clone(), image.clone(), check.clone())?;
  let saved = prepared.publish(main)?;
  let target = manager.root.join("vz").join(&machine.id);
  let original = std::fs::read(target.join("platform"))?;
  prepared.admit(main, &mut owner, media.clone())?;
  ensure!(
    owner.inspect(&machine.id)?.network_connected == Some(false),
    "Persisted offline policy was not applied to macOS hardware"
  );
  ensure!(
    vz::network::set_connected(&manager, &machine.id, true).is_err(),
    "Admitted macOS hardware allowed network policy replacement"
  );
  ensure!(
    owner.state(&machine.id)? == State::Stopped,
    "Admission unexpectedly started macOS"
  );
  ensure!(
    owner.inspect(&machine.id)?.sharing_devices == 1,
    "Persisted macOS folder was not attached"
  );
  ensure!(
    vz::sharing::remove(&manager, &machine.id, "work").is_err(),
    "Owned macOS hardware allowed folder removal"
  );
  let controller = service.clone();
  let id = machine.id.clone();
  let pending = runtime.spawn(async move {
    controller
      .transition(&id, Actor::Person, vz::Action::Start)
      .await
  });
  let deadline = Instant::now() + Duration::from_secs(30);
  while !pending.is_finished() {
    owner.tick();
    ensure!(
      Instant::now() < deadline,
      "Uninstalled-start rejection timed out"
    );
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  let error = runtime
    .block_on(pending)?
    .err()
    .context("Uninstalled macOS hardware started")?;
  ensure!(
    error.to_string().contains("Install macOS"),
    "Unexpected startup rejection: {error}"
  );
  let display = owner.display(&machine.id)?;
  ensure!(
    owner.display(&machine.id).is_err(),
    "Duplicate native display was accepted"
  );
  ensure!(
    owner.retire(&machine.id).is_err(),
    "Display lost runtime ownership"
  );
  drop(display);
  owner.retire(&machine.id)?;
  vz::network::set_connected(&manager, &machine.id, true)?;
  run_loop.runMode_beforeDate(
    unsafe { NSDefaultRunLoopMode },
    &NSDate::dateWithTimeIntervalSinceNow(0.01),
  );
  let mut prepared = platform::prepare(&manager, machine.clone(), image.clone(), check.clone())?;
  let reused = prepared.publish(main)?;
  ensure!(
    reused.identity == saved.identity && std::fs::read(target.join("platform"))? == original,
    "Repeated admission changed platform identity"
  );
  prepared.admit(main, &mut owner, media.clone())?;
  ensure!(
    owner.inspect(&machine.id)?.network_connected == Some(true),
    "Persisted NAT policy was not applied on macOS readmission"
  );
  let invalid = media.path().join("restore.ipsw");
  std::fs::write(&invalid, b"owned malformed restore media")?;
  for attempt in 0..4 {
    let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let access = allowed.clone();
    let original = check.clone();
    let scope: Check = Arc::new(move || {
      original()?;
      ensure!(
        access.load(std::sync::atomic::Ordering::Acquire),
        "Installer access revoked"
      );
      Ok(())
    });
    let installation = owner.install_checked(&machine.id, invalid.clone(), scope)?;
    ensure!(
      owner.active() && owner.inspect(&machine.id)?.busy,
      "Installation lost queue ownership"
    );
    ensure!(
      owner.can_replace(&machine.id).is_err(),
      "Active installation allowed hardware replacement"
    );
    if attempt == 3 {
      installation.cancel();
    }
    let installation = if attempt == 1 {
      drop(installation);
      None
    } else {
      if attempt == 2 {
        allowed.store(false, std::sync::atomic::Ordering::Release);
      }
      Some(installation)
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while owner.active() {
      owner.tick();
      ensure!(
        Instant::now() < deadline,
        "Owned malformed installation did not complete"
      );
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    if let Some(installation) = installation {
      ensure!(
        (0.0..=1.0).contains(&installation.fraction()),
        "Unbounded installer progress"
      );
      let error = runtime
        .block_on(installation.wait())
        .err()
        .context("Malformed installation succeeded")?;
      if attempt == 2 {
        ensure!(
          error.to_string().contains("revoked"),
          "Revocation was not retained: {error}"
        );
      }
    }
    let controller = service.clone();
    let id = machine.id.clone();
    let pending = runtime.spawn(async move {
      controller
        .transition(&id, Actor::Person, vz::Action::Start)
        .await
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !pending.is_finished() {
      owner.tick();
      ensure!(
        Instant::now() < deadline,
        "Post-installation Start rejection timed out"
      );
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    let error = runtime
      .block_on(pending)?
      .err()
      .context("Failed installation enabled Start")?;
    ensure!(
      error.to_string().contains("Install macOS"),
      "Unexpected startup failure: {error}"
    );
  }
  ensure!(
    std::fs::read(&invalid)? == b"owned malformed restore media",
    "Installer changed source media"
  );
  owner.retire(&machine.id)?;
  let prepared = platform::prepare(&manager, machine.clone(), image, check)?;
  manager.set_agent_access(&machine.id, false)?;
  ensure!(
    prepared.admit(main, &mut owner, media).is_err(),
    "Revoked original authority admitted macOS"
  );
  ensure!(
    std::fs::read(target.join("platform"))? == original,
    "Rejected admission changed persistent platform state"
  );
  println!("Signed native macOS platform publication, repeated stopped-hardware admission, identity/auxiliary binding, display ownership and uninstalled-start/revocation rejection, owned malformed installation, caller cancellation and installer revocation verified using supported SDK metadata; no valid local IPSW, installation, rendering/input or macOS first boot was verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native macOS platform diagnostic requires Apple silicon macOS")
}
