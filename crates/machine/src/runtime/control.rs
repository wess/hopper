use super::{variables, Boot, Control, Mode, Reason};
use crate::{devices::virtio::pci, hypervisor as hv};

pub struct Stopped {
  pub reason: Reason,
  /// last 64 UART bytes, retained locally for explicit boot diagnostics.
  pub console: Vec<u8>,
  pub variables: Vec<u8>,
  pub devices: [Option<pci::Device>; 7],
}

/// run on the VM owner thread; callbacks handle guest I/O and request a clean stop.
pub fn run(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  super::native::execute(boot, devices, None, None, None, None, poll)
}

pub fn run_persistent(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  anyhow::ensure!(
    boot.variables == variables::bytes(store),
    "Boot variables do not match their persistent store"
  );
  super::native::execute(boot, devices, Some(store), None, None, None, poll)
}

/// Keep normal firmware durability while allowing independent coordinated checkpoints.
/// The store remains exclusively borrowed through native completion and teardown.
pub fn run_persistent_captured(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  group: std::sync::Arc<hv::state::group::Group>,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  anyhow::ensure!(
    boot.variables == variables::bytes(store),
    "Boot variables do not match their persistent store"
  );
  super::native::execute(boot, devices, Some(store), Some(group), None, None, poll)
}

/// Captures CPU, software power/debug, RAM banks and GIC state while held.
/// Callbacks are deferred during collection, up to its five-second deadline.
/// Flash/UART and every current PCI backend, including disk/media clones, are included.
/// Each CPU owner also captures the exposed cycle-only PMU through a fixed private mapping.
/// Synchronous storage operations can outlast the collection deadline.
pub fn run_captured(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  group: std::sync::Arc<hv::state::group::Group>,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  super::native::execute(boot, devices, None, Some(group), None, None, poll)
}

/// Reconstruct a decoded checkpoint into fresh hardware, initially held.
/// Boot supplies matching topology and timeout; captured firmware/variables replace its bytes.
/// The cycle-only PMU is restored at final resume, after rebasing the held timer epoch.
/// The caller owns durable package admission and authorization. This is not an agent endpoint.
pub fn run_restored(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  checkpoint: &hv::state::group::Checkpoint,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  super::native::execute(boot, devices, None, None, Some(checkpoint), None, poll)
}

/// Resume a checkpoint with fresh captures while held or after a later coordinated pause.
/// Initial held capture retains the admitted CPU/PMU/clock and pending action state.
pub fn run_restored_captured(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  checkpoint: &hv::state::group::Checkpoint,
  group: std::sync::Arc<hv::state::group::Group>,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  super::native::execute(
    boot,
    devices,
    None,
    Some(group),
    Some(checkpoint),
    None,
    poll,
  )
}

/// A fresh, exclusively owned store must already contain the captured NVRAM bank.
/// Pending NOR operations remain in checkpoint command state and persist only on completion.
pub fn run_restored_persistent(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  checkpoint: &hv::state::group::Checkpoint,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  variables::matches(store, &checkpoint.primary.memory.nvram)?;
  super::native::execute(
    boot,
    devices,
    Some(store),
    None,
    Some(checkpoint),
    None,
    poll,
  )
}

pub fn run_restored_persistent_captured(
  boot: Boot,
  devices: [Option<pci::Device>; 7],
  store: &mut variables::Variables,
  checkpoint: &hv::state::group::Checkpoint,
  group: std::sync::Arc<hv::state::group::Group>,
  poll: impl FnMut(&mut [Option<pci::Device>; 7], &mut hv::Memory<'_>, Mode) -> anyhow::Result<Control>,
) -> anyhow::Result<Stopped> {
  variables::matches(store, &checkpoint.primary.memory.nvram)?;
  super::native::execute(
    boot,
    devices,
    Some(store),
    Some(group),
    Some(checkpoint),
    None,
    poll,
  )
}
