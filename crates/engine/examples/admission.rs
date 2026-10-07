#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    vz::{self, Action, MainThreadMarker, Service, Stage, State},
    Actor, Machines,
  };
  use fs2::FileExt;
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
  let record = engine::machines::linux::records::create(
    &manager,
    model::CreateMachine {
      name: "Admission diagnostic".into(),
      profile: "ubuntu".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 1,
        disk_gib: 10,
      },
      installer: Some(iso.to_str().context("ISO path must be UTF-8")?.into()),
      agent_access: true,
    },
  )?;
  ensure!(
    record.runtime == Some(model::MachineRuntime::Virtualization),
    "Native runtime choice was not persisted"
  );
  let id = record.id.as_str();
  vz::network::set_connected(&manager, id, false)?;
  vz::audio::set_speakers(&manager, id, false)?;
  let shared = root.path().join("shared");
  std::fs::create_dir(&shared)?;
  vz::sharing::add(
    &manager,
    id,
    model::MachineFolder {
      name: "work".into(),
      path: shared.to_str().context("Folder path must be UTF-8")?.into(),
      read_only: true,
    },
  )?;
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(1)
    .build()?;
  let (client, mut owner) = vz::channel();
  let service = Arc::new(Service::new(manager.clone(), client));
  let plan = engine::machines::linux::provision::prepare(
    id,
    &engine::machines::linux::provision::accounts(),
  )?;
  let seed_path = || -> anyhow::Result<std::path::PathBuf> {
    let seeds = std::fs::read_dir(manager.root.join("vz"))?
      .collect::<Result<Vec<_>, _>>()?
      .into_iter()
      .filter(|entry| entry.file_name().to_string_lossy().starts_with("seed"))
      .collect::<Vec<_>>();
    ensure!(seeds.len() == 1, "Expected one owned provisioning seed");
    Ok(seeds[0].path().join("seed"))
  };
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let prepared = prepared.provision(&plan)?;
  let cancelled_seed = seed_path()?;
  drop(prepared);
  ensure!(
    !cancelled_seed.exists(),
    "Cancelled preparation retained its seed"
  );
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let prepared = prepared.provision(&plan)?;
  let admission = prepared.admit(main, &mut owner)?;
  ensure!(
    owner.inspect(id)?.audio_devices == 0,
    "Muted guest admitted an audio device"
  );
  ensure!(
    vz::audio::set_speakers(&manager, id, true).is_err(),
    "Owned guest allowed speaker policy replacement"
  );
  ensure!(
    owner.inspect(id)?.network_connected == Some(false),
    "Persisted offline policy was not applied to Linux hardware"
  );
  ensure!(
    vz::network::set_connected(&manager, id, true).is_err(),
    "Admitted Linux hardware allowed network policy replacement"
  );
  ensure!(
    owner.inspect(id)?.sharing_devices == 1,
    "Persisted folder was not attached to native Linux hardware"
  );
  ensure!(
    vz::sharing::remove(&manager, id, "work").is_err(),
    "Owned hardware allowed folder removal"
  );
  ensure!(
    vz::sharing::set_read_only(&manager, id, "work", false).is_err(),
    "Owned hardware allowed folder access changes"
  );
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
    let identity = id.to_owned();
    let pending =
      runtime.spawn(async move { controller.transition(&identity, actor, action).await });
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
    let identity = id.to_owned();
    let pending = runtime.spawn(async move { query.status(&identity, Actor::Person).await });
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
  let display = owner.display(id)?;
  ensure!(
    owner.display(id).is_err(),
    "A VM must not acquire duplicate displays"
  );
  ensure!(
    owner.retire(id).is_err(),
    "A display must block ownership retirement"
  );
  unsafe {
    ensure!(
      display.view().virtualMachine().is_some(),
      "Display is not bound to admitted hardware"
    );
    ensure!(
      !display.view().capturesSystemKeys(),
      "System hotkeys must remain with the host"
    );
    ensure!(
      !display.view().automaticallyReconfiguresDisplay(),
      "Automatic resolution changes must remain disabled until teardown is supported"
    );
  }
  drop(display);
  owner.retire(id)?;
  vz::network::set_connected(&manager, id, true)?;
  vz::audio::set_speakers(&manager, id, true)?;
  vz::sharing::set_read_only(&manager, id, "work", false)?;
  lock.try_lock_exclusive()?;
  FileExt::unlock(&lock)?;
  let variables = std::fs::read(target.join("variables"))?;
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let prepared = prepared.provision(&plan)?;
  let admission = prepared.admit(main, &mut owner)?;
  ensure!(
    owner.inspect(id)?.audio_devices == 1,
    "Enabled speakers were not attached on readmission"
  );
  ensure!(
    owner.inspect(id)?.network_connected == Some(true),
    "Persisted NAT policy was not applied on Linux readmission"
  );
  drop(admission);
  ensure!(
    std::fs::read(target.join("identity"))? == identity,
    "Retry changed persistent VZ identity"
  );
  ensure!(
    std::fs::read(target.join("variables"))? == variables,
    "Retry changed persistent EFI state"
  );
  let display = owner.display(id)?;
  let retained = unsafe { display.view().virtualMachine() };
  ensure!(retained.is_some(), "Retry display lacks hardware");
  drop(retained);
  drop(owner);
  let seed = seed_path()?;
  ensure!(
    seed.exists(),
    "A surviving display lost its provisioning seed"
  );
  ensure!(
    lock.try_lock_exclusive().is_err(),
    "A surviving display lost runtime ownership"
  );
  drop(display);
  ensure!(
    !seed.exists(),
    "Released hardware retained its provisioning seed"
  );
  lock.try_lock_exclusive()?;
  FileExt::unlock(&lock)?;
  let rows = runtime.block_on(manager.list_non_windows(Actor::Person))?;
  ensure!(
    rows.len() == 1 && rows[0].state == "Ready to start" && !rows[0].busy,
    "Released native VM is not ready for admission"
  );
  let (client, mut owner) = vz::channel();
  let service = Service::new(manager.clone(), client);
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let prepared = prepared.provision(&plan)?;
  drop(prepared.admit(main, &mut owner)?);
  ensure!(
    std::fs::read(target.join("identity"))? == identity,
    "New owner changed persistent identity"
  );
  owner.retire(id)?;
  ensure!(
    !manager.root.join("lima").exists(),
    "Native admission must not invoke the previous helper"
  );
  println!("Native Linux admission, persisted identity/EFI reuse, provisioning seed cancellation/lifetime, runtime ownership, authorized status, native creation, repeated admission, exclusive display lifetime and agent lifecycle verified; no desktop installation was performed");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("VZ admission requires Apple silicon macOS")
}
