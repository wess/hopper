#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    linux::{
      observe,
      progress::{self, Phase},
      records,
    },
    vz::{self, Action, Service, Stage, State},
    Actor, Machines,
  };
  use std::{
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::Arc,
    time::{Duration, Instant},
  };

  let main = vz::MainThreadMarker::new().context("Handoff probe requires the main thread")?;
  let media = std::env::args()
    .nth(1)
    .context("Provide diagnostic ARM64 installation ISO")?;
  let root = tempfile::tempdir()?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let record = records::create(
    &manager,
    model::CreateMachine {
      name: "Handoff diagnostic".into(),
      profile: "ubuntu".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 1,
        disk_gib: 10,
      },
      agent_access: true,
      installer: Some(media),
    },
  )?;
  let id = &record.id;
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(1)
    .build()?;
  let (client, mut owner) = vz::channel();
  let service = Arc::new(Service::new(manager.clone(), client));
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
  let admission = prepared.admit(main, &mut owner)?;
  ensure!(
    owner.installer(id)?,
    "Installer admission was not protected"
  );
  let target = manager.root.join("vz").join(id);
  let identity = std::fs::read(target.join("identity"))?;
  pump(&runtime, &mut owner, runtime.spawn(admission.start()))?;
  ensure!(
    owner.can_replace(id).is_err(),
    "Running installer must not be replaced"
  );
  let controller = service.clone();
  let identity_id = id.clone();
  pump(
    &runtime,
    &mut owner,
    runtime.spawn(async move {
      controller
        .transition(&identity_id, Actor::Person, Action::Stop)
        .await
    }),
  )?;
  let display = owner.display(id)?;
  owner.can_replace(id)?;
  ensure!(
    owner.retire(id).is_err(),
    "Attached display must retain ownership"
  );
  drop(display);
  owner.retire(id)?;
  let mut disk = std::fs::OpenOptions::new()
    .write(true)
    .custom_flags(libc::O_NOFOLLOW)
    .open(target.join("disk"))?;
  disk.write_all(b"synthetic deployment diagnostic")?;
  disk.sync_all()?;
  let attempt = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let (mut output, observation) = observe::start(&target, attempt)?;
  writeln!(output, "HOPPER-INSTALL:{attempt}:installing")?;
  writeln!(output, "HOPPER-INSTALL:{attempt}:deployed")?;
  let deadline = Instant::now() + Duration::from_secs(5);
  while progress::read(&target)? != Some(Phase::Deployed) {
    ensure!(
      Instant::now() < deadline,
      "Diagnostic deployment marker was not saved"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  drop(output);
  drop(observation);
  let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Launch))?;
  ensure!(
    prepared.stage() == Stage::System,
    "Completed launch retained installation media"
  );
  let admission = prepared.admit(main, &mut owner)?;
  ensure!(!owner.installer(id)?, "System boot remained an installer");
  ensure!(
    service.installation(id, Actor::Person)? == Some(Phase::SystemBoot),
    "System boot intent was not saved"
  );
  ensure!(
    std::fs::read(target.join("identity"))? == identity,
    "Handoff changed persistent identity"
  );
  let display = owner.display(id)?;
  unsafe {
    ensure!(
      display.view().virtualMachine().is_some(),
      "Replacement display is not bound"
    );
  }
  pump(&runtime, &mut owner, runtime.spawn(admission.start()))?;
  ensure!(
    owner.state(id)? == State::Running,
    "Replacement hardware did not start"
  );
  let controller = service.clone();
  let identity_id = id.clone();
  pump(
    &runtime,
    &mut owner,
    runtime.spawn(async move {
      controller
        .transition(&identity_id, Actor::Person, Action::Stop)
        .await
    }),
  )?;
  drop(display);
  owner.retire(id)?;
  ensure!(
    runtime
      .block_on(service.prepare_linux(id, Actor::Person, Stage::Launch))?
      .stage()
      == Stage::System,
    "Restart lost system boot intent"
  );
  println!("Native stopped-installer replacement, display rebinding, identity reuse and persistent system-boot intent verified with a synthetic deployment marker; no Ubuntu installation or OS boot was verified");
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn pump(
  runtime: &tokio::runtime::Runtime,
  owner: &mut machine::vz::queue::Owner,
  pending: tokio::task::JoinHandle<anyhow::Result<()>>,
) -> anyhow::Result<()> {
  use anyhow::ensure;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::time::{Duration, Instant};
  let run_loop = NSRunLoop::currentRunLoop();
  let deadline = Instant::now() + Duration::from_secs(30);
  while !pending.is_finished() {
    owner.tick();
    ensure!(
      Instant::now() < deadline,
      "Native handoff transition timed out"
    );
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  runtime.block_on(pending)?
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Native handoff requires Apple silicon macOS")
}
