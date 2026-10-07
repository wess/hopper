#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    vz::{self, Action, MainThreadMarker, Service, Stage, State},
    Actor, Machines,
  };
  use fs2::FileExt;
  use model::{GuestOs, Machine};
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    fs::OpenOptions,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
  };

  let main = MainThreadMarker::new().context("VZ admission probe requires the main thread")?;
  let iso = PathBuf::from(
    std::env::args()
      .nth(1)
      .context("Pass a Linux ARM64 installation ISO")?,
  );
  let root = tempfile::tempdir()?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let id = "00000000-0000-0000-0000-000000000001";
  let record = Machine {
    id: id.into(),
    name: "Admission diagnostic".into(),
    guest: GuestOs::Linux,
    profile: "ubuntu".into(),
    resources: model::EngineResources {
      cpus: 2,
      memory_gib: 1,
      disk_gib: 10,
    },
    agent_access: true,
    agent_generation: 0,
    installer: Some(iso.to_str().context("ISO path must be UTF-8")?.into()),
  };
  store::json::write(
    &manager.root.join("records").join(format!("{id}.json")),
    &record,
  )?;
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(1)
    .build()?;
  let (client, mut owner) = vz::channel();
  let service = Arc::new(Service::new(manager.clone(), client));
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let admission = prepared.admit(main, &mut owner)?;
  let target = manager.root.join("vz").join(id);
  let identity = std::fs::read(target.join("identity"))?;
  let lock = OpenOptions::new()
    .read(true)
    .write(true)
    .open(manager.root.join("locks").join(format!("{id}.runtime")))?;
  ensure!(
    lock.try_lock_exclusive().is_err(),
    "Admitted VZ machine must hold runtime ownership"
  );
  let run_loop = NSRunLoop::currentRunLoop();
  let pending = runtime.spawn(admission.start());
  let deadline = Instant::now() + Duration::from_secs(30);
  while !pending.is_finished() {
    owner.tick();
    ensure!(Instant::now() < deadline, "VZ admission start timed out");
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  runtime.block_on(pending)??;
  ensure!(
    owner.state(id)? == State::Running,
    "Admitted VZ machine did not start"
  );
  for (actor, action) in [
    (Actor::Agent, Action::Pause),
    (Actor::Person, Action::Resume),
    (Actor::Person, Action::Stop),
  ] {
    if matches!(action, Action::Resume) {
      manager.set_agent_access(id, false)?;
      ensure!(
        runtime
          .block_on(service.transition(id, Actor::Agent, action))
          .is_err(),
        "Revoked guest agent must not resume native hardware"
      );
    }
    let controller = service.clone();
    let pending = runtime.spawn(async move { controller.transition(id, actor, action).await });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !pending.is_finished() {
      owner.tick();
      ensure!(
        Instant::now() < deadline,
        "VZ admission lifecycle timed out"
      );
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    runtime.block_on(pending)??;
    let expected = match action {
      Action::Pause => State::Paused,
      Action::Resume => State::Running,
      _ => State::Stopped,
    };
    ensure!(
      owner.state(id)? == expected,
      "VZ state differs from acknowledged lifecycle"
    );
    let query = service.clone();
    let pending = runtime.spawn(async move { query.status(id, Actor::Person).await });
    let deadline = Instant::now() + Duration::from_secs(30);
    while !pending.is_finished() {
      owner.tick();
      ensure!(Instant::now() < deadline, "VZ status timed out");
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    let status = runtime
      .block_on(pending)??
      .context("Admitted VM status is absent")?;
    ensure!(
      status.state == expected && !status.busy,
      "Authorized status differs from actual hardware"
    );
  }
  owner.retire(id)?;
  lock.try_lock_exclusive()?;
  FileExt::unlock(&lock)?;
  let variables = std::fs::read(target.join("variables"))?;
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let admission = prepared.admit(main, &mut owner)?;
  drop(admission);
  ensure!(
    std::fs::read(target.join("identity"))? == identity,
    "Retry changed persistent VZ identity"
  );
  ensure!(
    std::fs::read(target.join("variables"))? == variables,
    "Retry changed persistent EFI state"
  );
  owner.retire(id)?;
  ensure!(
    !manager.root.join("lima").exists(),
    "Native admission must not invoke the previous helper"
  );
  println!("Native Linux admission, persisted identity/EFI reuse, runtime ownership, authorized status and agent lifecycle verified; no desktop installation was performed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ admission requires Apple silicon macOS")
}
