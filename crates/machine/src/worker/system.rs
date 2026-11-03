use super::{
  boot,
  poll::Parent,
  restore::{self, Gate},
  snapshot,
};
use anyhow::{ensure, Context};
use machine::{
  runtime::{self, Control, Mode},
  snapshot::{
    generation::System,
    selection::{self, Phase},
  },
};
use model::native::{Boot, Restore, Result as Reply, StopReason};
use std::path::Path;

pub(super) fn run(
  mut parent: Parent,
  id: u64,
  config: Boot,
  restore: Restore,
) -> anyhow::Result<()> {
  let machine = model::native::machine_identity(&config.machine_id)
    .context("Invalid generation VM identity")?;
  ensure!(
    config.boot_media.is_none() && config.installer.is_none(),
    "Generation boot must detach setup media"
  );
  let path = Path::new(&config.store);
  ensure!(
    path.is_absolute() && path.file_name() == Some(std::ffi::OsStr::new("variables")),
    "Generation boot requires original VM namespace"
  );
  let root = restore::directory(path.parent().context("Missing VM namespace")?)?;
  let selected = selection::read_at(&root, machine)?;
  ensure!(
    selected.phase == Phase::Active
      && Some(selected.snapshot) == model::native::machine_identity(&restore.snapshot_id)
      && Some(selected.generation) == model::native::machine_identity(&restore.generation_id),
    "System boot differs from active generation selection"
  );
  let system = System::open(&root, selected, || Ok(()))?;
  let layout = system.layout();
  let (mut store, mut disks) = system.admit(|| Ok(()))?;
  ensure!(
    disks
      .iter()
      .enumerate()
      .all(|(slot, disk)| slot == 4 || disk.is_none()),
    "Native system boot only supports its saved system disk slot"
  );
  let disk_id: [u8; 20] = config
    .disk_id
    .as_bytes()
    .try_into()
    .context("Invalid current disk identity")?;
  let disk = disks[4]
    .take()
    .map(|disk| disk.disk(disk_id, || Ok(())))
    .transpose()?;
  let machine_id = config.machine_id.clone();
  let namespace = config.store.clone();
  let (mut boot, devices, configuration) = boot::current(config, disk)?;
  ensure!(
    configuration == selected.configuration,
    "Current generation hardware configuration differs"
  );
  ensure!(
    layout.cpus == boot.cpus as u8
      && layout.banks
        == [
          (0, 0x4000000),
          (0x4000000, 0x4000000),
          (machine::platform::RAM, boot.memory)
        ]
      && layout.devices
        == devices.each_ref().map(|device| device
          .as_ref()
          .map_or(0, machine::devices::virtio::pci::snapshot::records::kind)),
    "Current generation topology differs"
  );
  boot.variables = runtime::variables::bytes(&store).to_vec();
  let captures = machine::hypervisor::state::group::create(boot.cpus as usize)?;
  parent.snapshots = Some(snapshot::Snapshots::new(
    &namespace,
    &machine_id,
    configuration,
    captures.clone(),
  )?);
  parent.held = true;
  parent.restore = Some(Gate::active(root, restore.clone(), selected)?);
  let mut initialized = false;
  let stopped =
    runtime::run_persistent_captured(boot, devices, &mut store, captures, |devices, ram, mode| {
      if !initialized {
        // the fresh runtime polls before entering guest code; hold every CPU before acknowledgement.
        if mode != Mode::Paused {
          return Ok(Control::Pause);
        }
        parent.reply(
          id,
          Reply::GenerationStarted {
            machine_id: machine_id.clone(),
            configuration,
            restore: restore.clone(),
          },
          &[],
        )?;
        initialized = true;
      }
      parent.poll(devices, ram, mode)
    })?;
  ensure!(
    runtime::variables::bytes(&store) == stopped.variables,
    "Current firmware durability differs"
  );
  let reason = match stopped.reason {
    runtime::Reason::Stopped => StopReason::Requested,
    runtime::Reason::Shutdown => StopReason::Shutdown,
    runtime::Reason::Reset => StopReason::Reset,
  };
  super::drain(&mut parent)?;
  parent.reply(parent.stopping.unwrap_or(0), Reply::Stopped { reason }, &[])?;
  super::drain(&mut parent)?;
  Ok(())
}
