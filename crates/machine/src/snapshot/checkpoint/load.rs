use super::open;
use crate::{
  devices::{
    flash::snapshot::wire as flashwire,
    rtc, serial,
    virtio::pci::{self, snapshot::records as devicefiles},
  },
  hypervisor::{
    gic, ram,
    state::group::{wire, Checkpoint},
  },
  snapshot::{manifest::Record, native, package::Package, wire as softwarewire},
};
use anyhow::ensure;

/// Decode everything before allocating/restoring hardware. Every consumed record is admitted.
/// Expected device identities come from fresh, independently authorized configuration.
pub fn load(
  package: &Package,
  hardware: &[Option<pci::Device>; 7],
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<Checkpoint> {
  authorize()?;
  let authorize = std::cell::RefCell::new(authorize);
  let check = || (authorize.borrow_mut())();
  let layout = package.layout();
  ensure!(
    layout.devices
      == hardware
        .each_ref()
        .map(|device| device.as_ref().map_or(0, devicefiles::kind)),
    "Snapshot device topology mismatch"
  );
  let count = layout.cpus as usize;
  let expected = |slot: usize| -> anyhow::Result<(u64, usize)> {
    let (address, size) = layout.banks[slot];
    Ok((address, size.try_into()?))
  };
  let mut reopened = std::array::from_fn(|_| None);
  for (index, target) in hardware.iter().enumerate() {
    if let Some(target) = target {
      let bytes = open(package, Record::Transport(index as u8), check)?;
      let transport = match devicefiles::format(target) {
        devicefiles::Format::Bytes(_) => {
          let backend = open(package, Record::Backend(index as u8), check)?;
          devicefiles::decode(
            &bytes,
            devicefiles::Contents::Bytes(&backend),
            target,
            check,
          )?
        }
        devicefiles::Format::Graphics => {
          let mut file = package.reader(Record::Backend(index as u8), check)?;
          let transport = devicefiles::decode(
            &bytes,
            devicefiles::Contents::Graphics(&mut file),
            target,
            check,
          )?;
          file.finish()?;
          transport
        }
        devicefiles::Format::Image => {
          let stage = package.image(index as u8, check)?;
          let backend = open(package, Record::Backend(index as u8), check)?;
          devicefiles::decode(
            &bytes,
            devicefiles::Contents::Image {
              directory: &stage,
              bytes: &backend,
            },
            target,
            check,
          )?
        }
      };
      reopened[index] = Some(transport);
    }
  }
  let devices = native::Devices {
    flash: flashwire::decode(&open(package, Record::Flash, check)?)?,
    console: serial::wire::decode(&open(package, Record::Uart, check)?)?,
    rtc: rtc::wire::decode(&open(package, Record::Rtc, check)?)?,
    transports: reopened,
  };
  let softwarewire::State { software, actions } =
    softwarewire::decode(&open(package, Record::Software, check)?)?;
  let cpus = (0..count)
    .map(|index| {
      let bytes = open(package, Record::Cpu(index as u8), check)?;
      wire::decode(&bytes)
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  let interrupts = gic::state::wire::decode(&open(package, Record::Interrupts, check)?)?;
  let read = |slot: u8, expected| {
    let mut file = package.reader(Record::Memory(slot), check)?;
    let capture = ram::wire::decode(&mut file, expected, check)?;
    file.finish()?;
    Ok::<_, anyhow::Error>(capture)
  };
  let memory = native::Captured {
    firmware: read(0, expected(0)?)?,
    nvram: read(1, expected(1)?)?,
    ram: read(2, expected(2)?)?,
  };
  check()?;
  Ok(Checkpoint {
    cpus,
    primary: native::State {
      software,
      memory,
      interrupts,
      devices,
      actions,
    },
  })
}
