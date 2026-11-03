use super::Boot;
use crate::{
  acpi, debug,
  devices::{flash, rtc, serial, virtio::pci},
  hypervisor as hv, platform, psci,
};
use anyhow::ensure;

pub(super) struct Prepared<'a> {
  pub firmware: hv::Memory<'a>,
  pub nvram: hv::Memory<'a>,
  pub ram: hv::Memory<'a>,
  pub variables: flash::Flash,
  pub power: psci::power::Power,
  pub debug: Vec<debug::State>,
  pub console: serial::Console,
  pub rtc: rtc::Clock,
}

pub(super) fn validate(
  boot: &Boot,
  devices: &[Option<pci::Device>; 7],
  saved: Option<&hv::state::group::Checkpoint>,
) -> anyhow::Result<()> {
  super::validate(boot)?;
  if let Some(saved) = saved {
    ensure!(
      saved.cpus.len() == boot.cpus as usize
        && saved.primary.software.power().count() == saved.cpus.len(),
      "Captured CPU topology does not match"
    );
    for (memory, guest, size) in [
      (&saved.primary.memory.firmware, 0, 0x4000000),
      (&saved.primary.memory.nvram, 0x4000000, 0x4000000),
      (
        &saved.primary.memory.ram,
        platform::RAM,
        boot.memory as usize,
      ),
    ] {
      ensure!(
        memory.guest() == guest && memory.size() == size,
        "Captured memory topology does not match"
      );
    }
    ensure!(
      devices
        .iter()
        .zip(&saved.primary.devices.transports)
        .all(|(device, state)| device.is_some() == state.is_some()),
      "Captured device slots do not match"
    );
  }
  Ok(())
}

pub(super) fn create<'a>(
  vm: &'a hv::Vm,
  boot: &Boot,
  topology: &platform::Topology,
  devices: &mut [Option<pci::Device>; 7],
  saved: Option<&hv::state::group::Checkpoint>,
  mut disks: Option<&mut super::recovery::Disks>,
) -> anyhow::Result<Prepared<'a>> {
  let (mut firmware, mut nvram, mut ram) = if let Some(saved) = saved {
    (
      hv::ram::restore(vm, &saved.primary.memory.firmware)?,
      hv::ram::restore(vm, &saved.primary.memory.nvram)?,
      hv::ram::restore(vm, &saved.primary.memory.ram)?,
    )
  } else {
    (
      hv::memory(vm, 0, 0x4000000)?,
      hv::memory(vm, 0x4000000, 0x4000000)?,
      hv::memory(vm, platform::RAM, boot.memory as usize)?,
    )
  };
  let mut power = psci::power::create(
    &(0..boot.cpus as u64).collect::<Vec<_>>(),
    &[0..0x4000000, platform::RAM..platform::RAM + boot.memory],
  )?;
  let mut debug = (0..boot.cpus)
    .map(|_| debug::State::default())
    .collect::<Vec<_>>();
  let (variables, console) = if let Some(saved) = saved {
    let mut bytes = vec![0; 0x4000000];
    for (index, chunk) in bytes.chunks_mut(65536).enumerate() {
      saved.primary.memory.nvram.read(index * 65536, chunk)?;
    }
    let variables = flash::snapshot::restore(&saved.primary.devices.flash, bytes)?;
    saved.primary.software.restore(&mut power, &mut debug)?;
    for (slot, (device, state)) in devices
      .iter_mut()
      .zip(&saved.primary.devices.transports)
      .enumerate()
    {
      if let (Some(device), Some(state)) = (device, state) {
        let prepared = disks.as_mut().and_then(|disks| disks.files[slot].take());
        if let Some(prepared) = prepared {
          let disks = disks.as_mut().expect("retained disk authorization");
          pci::snapshot::restore_prepared(state, device, &ram, prepared, &mut disks.authorize)?;
        } else {
          pci::snapshot::restore(state, device, &ram)?;
        }
      }
    }
    if let Some(disks) = disks {
      ensure!(
        disks.files.iter().all(Option::is_none),
        "Unconsumed persistent disk slot"
      );
      (disks.authorize)()?;
    }
    (variables, saved.primary.devices.console.clone())
  } else {
    hv::write(&mut firmware, 0, &boot.firmware)?;
    let mut data = vec![0xff; 0x4000000];
    data[..boot.variables.len()].copy_from_slice(&boot.variables);
    let variables = flash::create(data, 0x40000)?;
    hv::write(&mut nvram, 0, flash::bytes(&variables))?;
    hv::write(&mut ram, 0, &platform::tree(topology)?)?;
    hv::write(
      &mut ram,
      (acpi::BASE - platform::RAM) as usize,
      &acpi::bundle(topology)?,
    )?;
    (variables, serial::Console::default())
  };
  hv::protect(&firmware, 5)?;
  hv::protect(&nvram, if flash::array(&variables) { 1 } else { 0 })?;
  Ok(Prepared {
    firmware,
    nvram,
    ram,
    variables,
    power,
    debug,
    console,
    rtc: match saved {
      Some(saved) => saved.primary.devices.rtc.clone(),
      None => rtc::create()?,
    },
  })
}

pub(super) fn hardware<'a>(
  vm: &'a hv::Vm,
  boot: &Boot,
) -> anyhow::Result<(hv::gic::Gic<'a>, platform::Topology)> {
  let gic = hv::gic::create_msi(
    vm,
    platform::DISTRIBUTOR,
    platform::REDISTRIBUTOR,
    platform::MSI,
    64,
    32,
  )?;
  let topology = platform::Topology {
    memory: boot.memory,
    cpus: boot.cpus,
    distributor_size: gic.distributor_size as u64,
    redistributor_size: gic.redistributor_size as u64,
    msi: gic.msi.as_ref().map(|msi| platform::Msi {
      size: msi.size as u64,
      first: msi.first,
      count: msi.count,
    }),
  };
  Ok((gic, topology))
}

pub(super) fn capture(
  group: &hv::state::group::Group,
  generation: u64,
  cpu: &mut hv::Cpu<'_>,
  pmu: hv::pmu::Token<'_>,
  firmware: &hv::Memory<'_>,
  state: &super::trap::State<'_, '_>,
  held: Option<&hv::state::group::Checkpoint>,
) -> anyhow::Result<()> {
  group.record_primary(generation, || {
    let actions = match held {
      Some(saved) => saved.primary.actions.clone(),
      None => {
        crate::snapshot::actions::capture(&state.tail, state.enter, std::time::Instant::now())?
      }
    };
    let software = {
      let power = state
        .power
        .lock()
        .map_err(|_| anyhow::anyhow!("CPU power lock poisoned"))?;
      crate::snapshot::Software::capture(&power, &state.debug)?
    };
    crate::snapshot::native::capture(
      software,
      firmware,
      state.nvram,
      state.ram,
      state.gic,
      crate::snapshot::native::Devices::capture(
        state.variables,
        &state.console,
        &state.rtc,
        state.devices,
      )?,
      actions,
    )
  })?;
  if let Some(saved) = held {
    return group.restored(generation, &saved.cpus);
  }
  // disk cloning can block; freeze the clock only after the held primary state
  // and GIC cut exist, before any owner is allowed to execute PMU instructions.
  group.arm(generation)?;
  group.record(cpu, 0, generation, pmu)
}
