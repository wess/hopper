use anyhow::{ensure, Context};
use engine::machines::{
  native::{self, config, restore::Owner, sessions::Sessions, State},
  Actor, Machines,
};
use machine::{
  devices::virtio::{block, console, gpu, input, pci},
  runtime::variables,
  snapshot::{checkpoint, manifest::Binding, package::Package},
};
use model::native::{Boot, Command, Restore, Result as Reply};
use std::{
  fs::{self, File, OpenOptions, Permissions},
  os::unix::fs::{FileExt, PermissionsExt},
  path::Path,
  sync::Arc,
  time::Duration,
};

pub async fn verify(
  sessions: &Sessions,
  manager: &Machines,
  paths: &config::Paths,
  helper: &Path,
  boot: Boot,
  restore: Restore,
  owned: Owner,
) -> anyhow::Result<()> {
  let id = boot.machine_id.clone();
  let selected = owned.selected()?.context("No active generation")?;
  let generation = paths
    .root
    .join("generations")
    .join(restore.generation_id.replace('-', ""));
  let mut store = variables::open(&generation.join("variables"))?;
  variables::commit(&mut store, 0, &[0x7f; 512])?;
  drop(store);
  let disk = generation.join("storage4/disk");
  let file = OpenOptions::new().read(true).write(true).open(&disk)?;
  file.write_all_at(b"current generation disk", 0)?;
  file.sync_all()?;
  drop(file);
  let original = fs::read(paths.variables.join("bank"))?;
  let journal = fs::read(paths.root.join("restore.hrr"))?;
  fs::rename(&paths.disk, paths.disk.with_extension("saved"))?;
  fs::rename(&paths.variables, paths.variables.with_extension("saved"))?;
  let owned = Arc::new(owned);
  let retained = Arc::downgrade(&owned);
  let worker =
    native::launch_generation(helper, boot.clone(), restore.clone(), owned.clone()).await?;
  ensure!(matches!(native::state(&worker), State::Paused));
  ensure!(native::request(&worker, Command::Resume {}).await.is_err());
  ensure!(matches!(
    native::request(&worker, Command::Status {}).await?,
    Reply::Status { paused: true, .. }
  ));
  super::held::verify(&worker, &owned, paths, &boot, &restore).await?;
  native::resume_generation(&worker, restore.clone(), owned.clone()).await?;
  ensure!(matches!(
    native::request(&worker, Command::Status {}).await?,
    Reply::Status { paused: false, .. }
  ));
  native::request(&worker, Command::Stop {}).await?;
  native::finished(&worker).await?;
  drop(worker);
  drop(owned);
  tokio::time::timeout(Duration::from_secs(2), async {
    while retained.upgrade().is_some() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await?;
  let owned = Owner::person(manager.clone(), &id)?;
  sessions
    .start_generation(&id, helper, boot.clone(), restore.clone(), owned)
    .await?;
  let watch = sessions.watch(&id).await?;
  ensure!(matches!(*watch.borrow(), State::Running));
  let capture = model::new_uuid();
  sessions
    .snapshot_named(&id, Actor::Person, &capture, "After cold boot")
    .await?;
  let hardware = [
    None,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    Some(pci::create(block::open(
      &disk,
      false,
      boot.disk_id.as_bytes().try_into()?,
    )?)?),
    None,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let layout = machine::snapshot::manifest::Layout {
    cpus: boot.cpus as u8,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, boot.memory),
    ],
    devices: [0, 2, 3, 3, 1, 0, 5],
  };
  let package = Package::open(
    &File::open(paths.root.join("snapshots"))?,
    Binding {
      snapshot: model::native::machine_identity(&capture).context("Cold capture identity")?,
      machine: selected.machine,
      configuration: selected.configuration,
    },
    layout,
    || Ok(()),
  )?;
  let saved = checkpoint::load(&package, &hardware, || Ok(()))?;
  let mut bytes = [0; 64];
  saved.primary.memory.ram.read(1 << 20, &mut bytes[..4])?;
  ensure!(
    bytes[..4] == 0x51u32.to_le_bytes(),
    "Released cold guest did not execute diagnostic RAM-writing firmware"
  );
  saved.primary.memory.nvram.read(0, &mut bytes)?;
  ensure!(
    bytes == [0x7f; 64],
    "Cold worker loaded original firmware variables"
  );
  drop(saved);
  drop(package);
  drop(hardware);
  sessions.stop(&id, Actor::Person).await?;
  ensure!(matches!(*watch.borrow(), State::Stopped(_)));
  ensure!(fs::read(paths.variables.with_extension("saved").join("bank"))? == original);
  ensure!(fs::read(paths.root.join("restore.hrr"))? == journal);
  let mut bytes = [0; 23];
  File::open(&disk)?.read_exact_at(&mut bytes, 0)?;
  ensure!(&bytes == b"current generation disk");
  let mut original_disk = [0; 20];
  File::open(paths.disk.with_extension("saved"))?.read_exact_at(&mut original_disk, 0)?;
  ensure!(&original_disk == b"original source disk");
  let receipt = generation.join("ownership.hgo");
  let good = fs::read(&receipt)?;
  let mut bad = good.clone();
  bad[4] ^= 1;
  fs::set_permissions(&receipt, Permissions::from_mode(0o600))?;
  fs::write(&receipt, &bad)?;
  fs::set_permissions(&receipt, Permissions::from_mode(0o400))?;
  let owned = Owner::person(manager.clone(), &id)?;
  ensure!(
    sessions
      .start_generation(&id, helper, boot.clone(), restore.clone(), owned)
      .await
      .is_err(),
    "Damaged ownership receipt executed a cold worker"
  );
  ensure!(fs::read(&receipt)? == bad && fs::read(paths.root.join("restore.hrr"))? == journal);
  fs::set_permissions(&receipt, Permissions::from_mode(0o600))?;
  fs::write(&receipt, &good)?;
  fs::set_permissions(&receipt, Permissions::from_mode(0o400))?;
  let owned = Owner::person(manager.clone(), &id)?;
  sessions
    .start_generation(&id, helper, boot, restore, owned)
    .await?;
  sessions.stop(&id, Actor::Person).await?;
  println!("Signed current-generation worker starts held, requires explicit resume, cold-boots without original disk/EFI paths, recaptures current firmware in the original VM namespace, preserves Active and original data, rejects damaged ownership without fallback and restarts after fixture recovery");
  Ok(())
}
