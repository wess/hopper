use machine::{
  devices::{pci, virtio::pci as vpci},
  hypervisor as hv,
};

pub fn deliver(
  gic: &hv::gic::Gic<'_>,
  index: usize,
  device: &mut vpci::Device,
) -> anyhow::Result<usize> {
  hv::gic::signal(
    gic,
    pci::interrupt(index as u8, 1)?,
    vpci::interrupt(device),
  )?;
  let messages = vpci::messages(device);
  let count = messages.len();
  for message in messages {
    hv::gic::message(gic, message.address, message.data)?;
  }
  Ok(count)
}
