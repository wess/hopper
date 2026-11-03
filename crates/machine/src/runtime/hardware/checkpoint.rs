use super::*;
use crate::snapshot::{manifest::Record, package::Package};

/// Rebuild immutable hardware shape without opening original mutable VM paths.
/// Caller must fully admit checkpoint components and attach recovered disks before execution.
pub fn checkpoint(
  config: model::native::Boot,
  package: &Package,
  expected: [u8; 32],
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], [u8; 32])> {
  authorize()?;
  ensure!(
    config.boot_media.is_none() && config.installer.is_none(),
    "Checkpoint recovery requires detached installation media"
  );
  ensure!(
    Some(package.binding().machine) == model::native::machine_identity(&config.machine_id)
      && package.binding().configuration == expected,
    "Checkpoint hardware binding differs"
  );
  let disk = if config.disk.is_some() {
    let mut reader = package.reader(Record::Backend(4), &mut authorize)?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    reader.finish()?;
    Some(block::snapshot::record::prototype(
      &bytes,
      config.disk_id.as_bytes().try_into()?,
    )?)
  } else {
    None
  };
  let result = current(config, disk, &mut authorize)?;
  let (boot, devices, configuration) = &result;
  let layout = crate::snapshot::manifest::Layout {
    cpus: boot.cpus.try_into()?,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (crate::platform::RAM, boot.memory),
    ],
    devices: devices
      .each_ref()
      .map(|device| device.as_ref().map_or(0, pci::snapshot::records::kind)),
  };
  ensure!(
    *configuration == expected && package.layout() == layout,
    "Rebuilt checkpoint hardware configuration/topology differs"
  );
  authorize()?;
  Ok(result)
}
