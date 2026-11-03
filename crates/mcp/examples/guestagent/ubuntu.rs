use anyhow::{ensure, Context};
use engine::machines::{
  vz::{Action, Stage},
  Actor,
};
use host::Host;
use machine::vz::{self, queue::Owner};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
use std::{
  os::unix::fs::DirBuilderExt,
  path::Path,
  sync::Arc,
  time::{Duration, Instant},
};
use tokio::runtime::Runtime;

fn wait<T: Send + 'static>(
  runtime: &Runtime,
  owner: &mut Owner,
  host: &Arc<Host>,
  id: &str,
  phase: &str,
  future: impl std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
) -> anyhow::Result<T> {
  let task = runtime.spawn(future);
  let deadline = Instant::now() + Duration::from_secs(45 * 60);
  let run_loop = NSRunLoop::currentRunLoop();
  let mut report = Instant::now();
  while !task.is_finished() {
    ensure!(
      Instant::now() < deadline,
      "Ubuntu diagnostic timed out during {phase}"
    );
    owner.tick();
    if Instant::now() >= report {
      eprintln!(
        "Ubuntu {phase}: {:?}",
        host.virtual_linux_installation(id, Actor::Person)?
      );
      report = Instant::now() + Duration::from_secs(30);
    }
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  runtime.block_on(task)?
}

fn install(root: &Path) -> anyhow::Result<()> {
  std::env::set_var("HOPPER_DIR", root);
  let host = Host::from_env();
  std::env::remove_var("HOPPER_DIR");
  let runtime = Runtime::new()?;
  let id = "f2db7404-ff91-4c96-b135-bf1b4a06095a";
  let manager = host.machines();
  std::fs::create_dir_all(manager.root.join("records"))?;
  let record = model::Machine {
    id: id.into(),
    name: "Isolated installed Ubuntu agent diagnostic".into(),
    guest: model::GuestOs::Linux,
    profile: "ubuntu".into(),
    resources: model::EngineResources {
      cpus: 4,
      memory_gib: 4,
      disk_gib: 64,
    },
    installer: None,
    agent_access: true,
    agent_generation: 0,
    runtime: Some(model::MachineRuntime::Virtualization),
  };
  std::fs::write(
    manager.root.join("records").join(format!("{id}.json")),
    serde_json::to_vec(&record)?,
  )?;
  let main = vz::MainThreadMarker::new().context("Ubuntu probe requires the main thread")?;
  let mut owner = host.virtual_machine_owner()?;
  {
    let _enter = runtime.enter();
    host.serve_machine_agents()?;
  }
  let prepare = host.clone();
  let prepared = wait(&runtime, &mut owner, &host, id, "preparation", async move {
    prepare
      .prepare_virtual_linux(id, Actor::Person, Stage::Launch)
      .await
  })?;
  let admission = prepared.admit(main, &mut owner)?;
  let installation = admission
    .installation()
    .context("Missing unattended installation watch")?;
  wait(
    &runtime,
    &mut owner,
    &host,
    id,
    "installer start",
    admission.start(),
  )?;
  let installation = wait(
    &runtime,
    &mut owner,
    &host,
    id,
    "installation",
    installation.wait(),
  )?;
  let permit = installation.permit(&owner)?;
  permit.retire(&mut owner)?;
  let prepared = wait(
    &runtime,
    &mut owner,
    &host,
    id,
    "system preparation",
    permit.prepare(),
  )?;
  let admission = prepared.admit(main, &mut owner)?;
  wait(
    &runtime,
    &mut owner,
    &host,
    id,
    "system start",
    admission.start(),
  )?;
  let result = wait(
    &runtime,
    &mut owner,
    &host,
    id,
    "installed MCP checks",
    super::probe::exercise(host.clone(), root.to_owned(), id),
  );
  let stop = host.clone();
  let stopped = wait(&runtime, &mut owner, &host, id, "system stop", async move {
    stop
      .virtual_machine_lifecycle(id, Actor::Person, Action::Stop)
      .await
  });
  result?;
  stopped?;
  owner.retire(id)?;
  println!("Production Ubuntu acquisition, unattended installation, system admission and installed MCP checks verified");
  Ok(())
}

pub(super) fn run() -> anyhow::Result<()> {
  let root = tempfile::Builder::new()
    .prefix("hopperubuntu90.")
    .tempdir_in("/tmp")?
    .keep();
  eprintln!("Isolated Ubuntu evidence and media: {}", root.display());
  let args = std::env::args_os().skip(2).collect::<Vec<_>>();
  ensure!(
    args.len() <= 1,
    "Provide at most one previously acquired Ubuntu ISO"
  );
  if let Some(source) = args.first() {
    let source = Path::new(source);
    let metadata = std::fs::symlink_metadata(source)?;
    ensure!(
      source.is_absolute() && metadata.is_file() && metadata.len() == engine::machines::linux::SIZE,
      "Cached diagnostic media must be an absolute regular pinned-size ISO"
    );
    let cache = root.join("machines/native/images/linux");
    std::fs::DirBuilder::new()
      .recursive(true)
      .mode(0o700)
      .create(&cache)?;
    let destination = cache.join(format!("{}.iso", engine::machines::linux::SHA256));
    ensure!(
      std::process::Command::new("/bin/cp")
        .arg("-c")
        .arg(source)
        .arg(&destination)
        .status()?
        .success(),
      "Clone the previously acquired installer into the private diagnostic cache"
    );
    eprintln!("Reusing acquired media through production size/checksum verification");
  }
  install(&root)
}
