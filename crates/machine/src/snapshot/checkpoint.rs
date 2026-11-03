//! complete native checkpoint records; the caller supplies authorized identity/configuration.
//! successful loading does not start hardware or establish desktop/application recovery.

use super::{manifest::Layout, package::Package};
use crate::{devices::virtio::pci::snapshot::records, hypervisor::state::group::Checkpoint};
mod load;
mod save;
pub use load::load;
pub use save::{save, stage};

pub fn layout(saved: &Checkpoint) -> anyhow::Result<Layout> {
  let memory = &saved.primary.memory;
  let layout = Layout {
    cpus: saved.cpus.len().try_into()?,
    banks: [&memory.firmware, &memory.nvram, &memory.ram]
      .map(|bank| (bank.guest(), bank.size() as u64)),
    devices: saved
      .primary
      .devices
      .transports
      .each_ref()
      .map(|state| state.as_ref().map_or(0, records::saved_kind)),
  };
  layout.records()?;
  Ok(layout)
}

fn open(
  package: &Package,
  record: super::manifest::Record,
  authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<zeroize::Zeroizing<Vec<u8>>> {
  use std::io::Read;
  let mut file = package.reader(record, authorize)?;
  let mut bytes = zeroize::Zeroizing::new(Vec::new());
  file.read_to_end(&mut bytes)?;
  file.finish()?;
  Ok(bytes)
}
