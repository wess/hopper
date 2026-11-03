#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/admission.rs"]
mod admission;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/agent.rs"]
mod agent;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/automatic.rs"]
mod automatic;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/direct.rs"]
mod direct;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/fallback.rs"]
mod fallback;
#[path = "snapshotcontroller/held.rs"]
mod held;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/integrity.rs"]
mod integrity;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/paused.rs"]
mod paused;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/registry.rs"]
mod registry;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/retained.rs"]
mod retained;
#[path = "snapshotcontroller/rollback.rs"]
mod rollback;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "snapshotcontroller/system.rs"]
mod system;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
async fn run() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use engine::machines::{
    native::{config, sessions::Sessions, snapshot::Policy},
    Actor, Machines,
  };
  use machine::{
    devices::virtio::{block, console, gpu, input, pci},
    snapshot::{
      checkpoint, configuration,
      manifest::{Binding, Layout},
      package::{inventory, Package},
    },
  };
  use model::native::{Boot, Command, Result as Reply};
  use std::{fs::File, path::PathBuf, time::Duration};

  let args: Vec<_> = std::env::args_os().skip(1).collect();
  ensure!(
    args.len() == 3,
    "Expected private fixture, machine identity and signed worker"
  );
  let root = PathBuf::from(&args[0]);
  let id = args[1].to_str().context("Invalid machine identity")?;
  let manager = Machines {
    root: root.join("machines"),
  };
  if std::env::var("HOPPER_RESTORE_ROLLBACK_CHECK").as_deref() == Ok("1") {
    return rollback::resume(&manager, id, &PathBuf::from(&args[2])).await;
  }
  let paths = config::initialize(&manager, id)?;
  let configured = std::env::var("HOPPER_RESTORE_CONFIGURED").as_deref() == Ok("1");
  let memory = if configured { 1 << 30 } else { 0x10000000 };
  let disk_id = if configured {
    use sha2::{Digest, Sha256};
    let digest = format!(
      "{:x}",
      Sha256::digest(uuid::Uuid::parse_str(id)?.as_bytes())
    );
    format!("hpr{}", &digest[..17])
  } else {
    "hopper00000000000001".into()
  };
  if configured {
    use std::os::unix::fs::PermissionsExt;
    let path = paths.root.join("installation");
    // Synthetic firmware has no Windows installer; this receipt only admits the API probe.
    std::fs::write(
      &path,
      serde_json::to_vec(&serde_json::json!({
        "version": 1, "vmId": id, "installation": model::native::Installation::SystemStarted {},
      }))?,
    )?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
  }
  let with_disk = std::env::var("HOPPER_RESTORE_REGISTRY").as_deref() == Ok("1");
  if with_disk {
    use std::os::unix::fs::{FileExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .open(&paths.disk)?;
    file.set_len(if configured { 64 << 30 } else { 64 << 20 })?;
    file.write_all_at(b"original source disk", 0)?;
    file.sync_all()?;
  }
  let sessions = Sessions::new(manager.clone());
  let boot = Boot {
    machine_id: id.into(),
    firmware: root.join("firmware").to_string_lossy().into(),
    variables: root.join("template").to_string_lossy().into(),
    store: paths.variables.to_string_lossy().into(),
    boot_media: None,
    installer: None,
    disk: with_disk.then(|| paths.disk.to_string_lossy().into()),
    disk_id: disk_id.clone(),
    memory,
    cpus: 2,
    timeout_ms: if configured { None } else { Some(30000) },
  };
  let restored_boot = boot.clone();
  sessions.start(id, &PathBuf::from(&args[2]), boot).await?;
  let _server = sessions.serve_agents()?;
  let snapshot = model::new_uuid();
  engine::machines::native::sessions::remote::snapshots::capture(
    &manager,
    id,
    0,
    &snapshot,
    "Before updates",
  )
  .await?;
  let Reply::Status { paused, .. } = sessions
    .request(id, Actor::Person, Command::Status {})
    .await?
  else {
    anyhow::bail!("Snapshot source returned no status");
  };
  ensure!(!paused, "Snapshot did not resume its running source");
  let hardware = [
    None,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    if with_disk {
      Some(pci::create(block::open(
        &paths.disk,
        false,
        disk_id.as_bytes().try_into()?,
      )?)?)
    } else {
      None
    },
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let expected = machine::runtime::Boot {
    firmware: std::fs::read(root.join("firmware"))?,
    variables: vec![0xff],
    memory,
    cpus: 2,
    timeout: Duration::MAX,
  };
  let binding = Binding {
    machine: model::native::machine_identity(id).context("Invalid identity")?,
    snapshot: model::native::machine_identity(&snapshot).context("Invalid snapshot identity")?,
    configuration: configuration::fingerprint(id, &expected, &hardware, || Ok(()))?,
  };
  let layout = Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, memory),
    ],
    devices: [0, 2, 3, 3, if with_disk { 1 } else { 0 }, 0, 5],
  };
  let parent = File::open(paths.root.join("snapshots"))?;
  let policy = Policy::agent(manager.clone(), id, 0)?;
  let discovered = inventory(&parent, binding.machine, || policy.check())?;
  ensure!(
    discovered.len() == 1 && discovered[0].binding == binding && discovered[0].layout == layout,
    "Committed snapshot was not discovered with its original identity and hardware"
  );
  let package = Package::open(&parent, binding, layout, || policy.check())?;
  ensure!(
    discovered[0]
      .description
      .as_ref()
      .is_some_and(|description| description.name() == "Before updates"),
    "Snapshot name was not committed with its package"
  );
  let saved = checkpoint::load(&package, &hardware, || policy.check())?;
  ensure!(saved.cpus.len() == 2, "Snapshot lost a CPU");
  drop(saved);
  drop(package);
  let manifest = std::fs::read(
    paths
      .root
      .join("snapshots")
      .join(snapshot.replace('-', ""))
      .join("manifest.hsm"),
  )?;
  ensure!(
    sessions
      .snapshot(id, Actor::Agent, &snapshot)
      .await
      .is_err(),
    "Duplicate snapshot replaced its target"
  );
  ensure!(
    std::fs::read(
      paths
        .root
        .join("snapshots")
        .join(snapshot.replace('-', ""))
        .join("manifest.hsm")
    )? == manifest,
    "Duplicate snapshot changed its manifest"
  );
  manager.set_agent_access(id, false)?;
  ensure!(
    sessions
      .snapshot(id, Actor::Agent, &model::new_uuid())
      .await
      .is_err(),
    "Revoked agent captured a snapshot"
  );
  let rollback = model::new_uuid();
  sessions
    .request(id, Actor::Person, Command::Pause {})
    .await?;
  sessions.snapshot(id, Actor::Person, &rollback).await?;
  ensure!(
    matches!(
      sessions
        .request(id, Actor::Person, Command::Status {})
        .await?,
      Reply::Status { paused: true, .. }
    ),
    "Snapshot resumed a manually paused source"
  );
  sessions
    .request(id, Actor::Person, Command::Resume {})
    .await?;
  if std::env::var("HOPPER_RESTORE_AUTOMATIC").as_deref() == Ok("1") {
    drop(hardware);
    return automatic::verify(
      &sessions,
      &manager,
      &paths,
      &PathBuf::from(&args[2]),
      restored_boot,
      id,
      &snapshot,
    )
    .await;
  }
  sessions.stop(id, Actor::Person).await?;
  ensure!(
    sessions.state(id, Actor::Person).await?.is_none(),
    "Source ownership was not retired"
  );
  let policy = Policy::person(manager.clone(), id)?;
  let catalog = policy.inventory()?;
  ensure!(
    catalog.iter().any(|entry| entry.id == snapshot
      && entry.name.as_deref() == Some("Before updates")
      && entry.created_unix_seconds.is_some()),
    "Named capture did not survive worker teardown"
  );
  ensure!(
    catalog.len() == 2,
    "Durable catalog lost saved captures after worker teardown"
  );
  ensure!(
    inventory(&parent, binding.machine, || policy.check())?.len() == 2,
    "Committed snapshots were not discovered after worker teardown"
  );
  let owner = engine::machines::native::restore::Owner::person(manager.clone(), id)?;
  drop(hardware);
  let prepared =
    engine::machines::native::restore::prepare(owner, restored_boot, &snapshot, &rollback).await?;
  let owner = prepared.owner;
  let restored_boot = prepared.boot;
  let restore = prepared.restore;
  let generation = model::native::machine_identity(&restore.generation_id)
    .context("Prepared generation identity")?;
  if std::env::var("HOPPER_RESTORE_REGISTRY").as_deref() == Ok("1") {
    return registry::verify(
      &sessions,
      &manager,
      &paths,
      &PathBuf::from(&args[2]),
      restored_boot,
      owner,
      restore,
    )
    .await;
  }
  let original_boot = restored_boot.clone();
  let owner = direct::verify(
    &manager,
    &paths,
    &PathBuf::from(&args[2]),
    restored_boot,
    owner,
    restore,
  )
  .await?;
  let source = Package::open(&parent, binding, layout, || owner.check())?;
  let generations = File::open(paths.root.join("generations"))?;
  let (_, hardware, _) = machine::runtime::hardware::original(original_boot, || owner.check())?;
  ensure!(machine::snapshot::generation::Generation::open(
    &generations,
    &source,
    &hardware,
    generation,
    || owner.check()
  )
  .is_err());
  ensure!(owner
    .selected()?
    .is_some_and(|record| record.phase == engine::machines::native::restore::Phase::Active));
  ensure!(config::initialize(&manager, id).is_err());
  println!("Signed host restore selection binds real source/prior captures and a prepared EFI generation, persists attempt intent before core consumption, activates before execution, survives owner teardown and blocks original startup/replay");
  println!("Signed host controller retained real worker/session ownership through capture, decoded its committed checkpoint, rejected replacement/revocation and preserved a manually paused source");
  Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  return run().await;
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  anyhow::bail!("Native snapshot controller checks require Apple silicon macOS");
}
