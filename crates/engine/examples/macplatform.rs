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
    owner.state(&machine.id)? == State::Stopped,
    "Admission unexpectedly started macOS"
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
  println!("Signed native macOS platform publication, repeated stopped-hardware admission, identity/auxiliary binding, display ownership and uninstalled-start/revocation rejection verified using supported SDK metadata; no valid local IPSW, installation, rendering/input or macOS first boot was verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native macOS platform diagnostic requires Apple silicon macOS")
}
