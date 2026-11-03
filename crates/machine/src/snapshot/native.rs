//! primary-owned checkpoint data; durable disk publication and full runtime recovery are pending.

use super::{actions, Software};
use crate::{
  devices::{flash, rtc, serial, virtio::pci},
  hypervisor::{gic, ram, Memory},
};

pub struct State {
  pub software: Software,
  pub memory: Captured,
  pub interrupts: gic::state::State,
  pub devices: Devices,
  pub actions: actions::State,
}

pub struct Devices {
  pub flash: flash::snapshot::State,
  pub console: serial::Console,
  pub rtc: rtc::Clock,
  pub transports: [Option<pci::snapshot::State>; 7],
}

impl Devices {
  pub fn capture(
    flash: &flash::Flash,
    console: &serial::Console,
    rtc: &rtc::Clock,
    devices: &[Option<pci::Device>; 7],
  ) -> anyhow::Result<Self> {
    let mut transports = std::array::from_fn(|_| None);
    for (index, device) in devices.iter().enumerate() {
      transports[index] = device.as_ref().map(pci::snapshot::capture).transpose()?;
    }
    Ok(Self {
      flash: flash::snapshot::capture(flash),
      console: console.clone(),
      rtc: rtc.clone(),
      transports,
    })
  }
}

pub struct Captured {
  pub firmware: ram::Capture,
  pub nvram: ram::Capture,
  pub ram: ram::Capture,
}

/// Called by the primary owner with every CPU and device writer held.
/// Any failure drops the partial captures instead of publishing a partial checkpoint.
pub fn capture(
  software: Software,
  firmware: &Memory<'_>,
  nvram: &Memory<'_>,
  memory: &Memory<'_>,
  interrupts: &gic::Gic<'_>,
  devices: Devices,
  actions: actions::State,
) -> anyhow::Result<State> {
  Ok(State {
    software,
    memory: Captured {
      firmware: ram::capture(firmware)?,
      nvram: ram::capture(nvram)?,
      ram: ram::capture(memory)?,
    },
    interrupts: gic::state::capture(interrupts)?,
    devices,
    actions,
  })
}
