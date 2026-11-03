use machine::{
  devices::virtio::{block, pci},
  runtime::{hardware, variables, Boot},
};
use std::path::Path;

pub(super) struct Prepared {
  pub boot: Boot,
  pub devices: [Option<pci::Device>; 7],
  pub store: variables::Variables,
  pub configuration: [u8; 32],
}

pub(super) fn prepare(config: model::native::Boot) -> anyhow::Result<Prepared> {
  let root = config.store.clone();
  let (mut boot, devices, configuration) = hardware(config)?;
  let root = Path::new(&root);
  let store = if root.exists() {
    variables::open(root)?
  } else {
    variables::create(root, &boot.variables)?
  };
  boot.variables = variables::bytes(&store).to_vec();
  Ok(Prepared {
    boot,
    devices,
    store,
    configuration,
  })
}

pub(super) fn hardware(
  config: model::native::Boot,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], [u8; 32])> {
  hardware::original(config, || Ok(()))
}

pub(super) fn current(
  config: model::native::Boot,
  disk: Option<block::Disk>,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], [u8; 32])> {
  hardware::current(config, disk, || Ok(()))
}
