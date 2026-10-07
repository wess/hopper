#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    linux::{observe, records},
    vz::{self, Action, Service, Stage, State},
    Actor, Machines,
  };
  use machine::vz::{Boot, Linux};
  use std::sync::Arc;

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 3,
    "Provide diagnostic ISO, ARM64 kernel and shutdown initramfs"
  );
  let main =
    vz::MainThreadMarker::new().context("Automatic handoff probe requires the main thread")?;
  let root = tempfile::tempdir()?;
  let manager = Machines {
    root: root.path().join("machines"),
  };
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .enable_all()
    .worker_threads(1)
    .build()?;
  let (client, mut owner) = vz::channel();
  let service = Arc::new(Service::new(manager.clone(), client));
  let attempt = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  for scenario in ["handoff", "stop", "revocation"] {
    let record = records::create(
      &manager,
      model::CreateMachine {
        name: format!("Automatic {scenario} diagnostic"),
        profile: "ubuntu".into(),
        resources: model::EngineResources {
          cpus: 2,
          memory_gib: 1,
          disk_gib: 10,
        },
        agent_access: true,
        installer: Some(args[0].to_str().context("ISO path must be UTF-8")?.into()),
      },
    )?;
    let id = &record.id;
    let prepared = runtime.block_on(service.prepare_linux(id, Actor::Person, Stage::Installer))?;
    drop(prepared.admit(main, &mut owner)?);
    owner.retire(id)?;
    let target = manager.root.join("vz").join(id);
    let identity: serde_json::Value =
      serde_json::from_reader(std::fs::File::open(target.join("identity"))?)?;
    let bytes = serde_json::from_value(identity["identity"].clone())?;
    let (output, observation) = observe::start(&target, attempt)?;
    let boot = Linux {
      cpus: 2,
      memory: 1 << 30,
      width: 1024,
      height: 768,
      identity: bytes,
      boot: Boot::Kernel {
        kernel: args[1].clone().into(),
        initramfs: Some(args[2].clone().into()),
        command_line: "console=hvc0 rdinit=/init".into(),
      },
      disk: target.join("disk"),
      installer: None,
      seed: None,
      network: None,
      shares: Vec::new(),
      console: Some(output),
    };
    let mut vm = machine::vz::create(main, &boot)?;
    machine::vz::restrict_installer_restart(&mut vm)?;
    machine::vz::retain(
      &mut vm,
      Arc::new((
        store::lock::exclusive(
          std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(manager.root.join("locks").join(format!("{id}.runtime")))?,
        )?,
        observation,
      )),
    )?;
    owner.insert(id, vm)?;
    let generation = owner.inspect(id)?.generation;
    let controller = service.clone();
    let identity_id = id.clone();
    let installation = pump(
      &runtime,
      &mut owner,
      runtime.spawn(async move {
        controller
          .watch_installation(&identity_id, Actor::Agent, attempt)
          .await
      }),
    )?;
    let display = owner.display(id)?;
    let controller = service.clone();
    let identity_id = id.clone();
    pump(
      &runtime,
      &mut owner,
      runtime.spawn(async move {
        controller
          .transition(&identity_id, Actor::Agent, Action::Start)
          .await
      }),
    )?;
    if scenario == "stop" {
      let controller = service.clone();
      let identity_id = id.clone();
      let _ = pump(
        &runtime,
        &mut owner,
        runtime.spawn(async move {
          controller
            .transition(&identity_id, Actor::Person, Action::Stop)
            .await
        }),
      );
      let result = pump(&runtime, &mut owner, runtime.spawn(installation.wait()));
      ensure!(
        result
          .err()
          .context("Explicit stop unexpectedly allowed automatic startup")?
          .to_string()
          .contains("explicit"),
        "Explicit stop did not cancel the original watch"
      );
      ensure!(
        owner.inspect(id)?.stop_requested,
        "Stop intent was not reflected by owned hardware"
      );
    } else {
      let installation = pump(&runtime, &mut owner, runtime.spawn(installation.wait()))?;
      ensure!(
        owner.state(id)? == State::Stopped && !owner.inspect(id)?.stop_requested,
        "Guest shutdown was confused with an explicit stop"
      );
      if scenario == "revocation" {
        manager.set_agent_access(id, false)?;
        manager.set_agent_access(id, true)?;
        ensure!(
          installation
            .permit(&owner)
            .err()
            .context("Re-enabled access revived an old watch")?
            .to_string()
            .contains("agent policy"),
          "Original agent policy was not preserved"
        );
      } else {
        let permit = installation.permit(&owner)?;
        drop(display);
        permit.retire(&mut owner)?;
        let prepared = runtime.block_on(permit.prepare())?;
        ensure!(
          prepared.stage() == Stage::System,
          "Automatic handoff retained installation media"
        );
        let admission = prepared.admit(main, &mut owner)?;
        ensure!(
          owner.inspect(id)?.generation != generation,
          "Replacement reused its runtime generation"
        );
        let display = owner.display(id)?;
        pump(&runtime, &mut owner, runtime.spawn(admission.start()))?;
        ensure!(
          owner.state(id)? == State::Running,
          "Automatic system hardware did not start"
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
        println!("Verified natural guest shutdown and automatic hardware handoff");
        continue;
      }
    }
    drop(display);
    ensure!(
      owner.state(id)? == State::Stopped,
      "Diagnostic guest did not stop"
    );
    owner.retire(id)?;
    println!("Verified automatic startup suppression: {scenario}");
  }
  println!("Diagnostic Linux guest wrote synthetic disk data and powered off; no Ubuntu installation, actual system boot, desktop rendering or app-window behavior was verified");
  Ok(())
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn pump<T>(
  runtime: &tokio::runtime::Runtime,
  owner: &mut machine::vz::queue::Owner,
  pending: tokio::task::JoinHandle<anyhow::Result<T>>,
) -> anyhow::Result<T> {
  use anyhow::ensure;
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::time::{Duration, Instant};
  let run_loop = NSRunLoop::currentRunLoop();
  let deadline = Instant::now() + Duration::from_secs(30);
  while !pending.is_finished() {
    owner.tick();
    ensure!(
      Instant::now() < deadline,
      "Automatic handoff diagnostic timed out"
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
  anyhow::bail!("Automatic handoff probe requires Apple silicon macOS")
}
