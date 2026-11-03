use crate::{
  debug,
  devices::{flash, serial},
  hypervisor::{
    self as hv,
    state::{clock, group},
  },
  psci::power,
  snapshot::{self, native},
};
use std::time::{Duration, Instant};

pub(super) fn verify(
  vm: &hv::Vm,
  cpu: &mut hv::Cpu<'_>,
  access: hv::pmu::Token<'_>,
  ram: &hv::Memory<'_>,
  gic: &hv::gic::Gic<'_>,
  offset: u64,
) -> anyhow::Result<()> {
  let firmware = hv::memory(vm, 0, 65536)?;
  let nvram = hv::memory(vm, 0x4000000, 65536)?;
  let variables = flash::create(vec![0xff; 65536], 4096)?;
  let power = power::create(&[0], &[0..65536, 0x40000000..0x40004000])?;
  let group = group::create(1)?;
  let _binding = group.bind(1)?;
  group.enter(1)?;
  let mut request = group.request()?;
  // arming and CPU collection must wait for the primary's complete cut.
  group.arm(1)?;
  group.record(cpu, 0, 1, access)?;
  anyhow::ensure!(request.poll()?.is_none());
  let mut finished = 0;
  group.record_primary(1, || {
    std::thread::sleep(Duration::from_millis(40));
    let saved = native::capture(
      snapshot::Software::capture(&power, &[debug::State::default()])?,
      &firmware,
      &nvram,
      ram,
      gic,
      native::Devices {
        flash: flash::snapshot::capture(&variables),
        console: serial::Console::default(),
        rtc: crate::devices::rtc::create()?,
        transports: std::array::from_fn(|_| None),
      },
      snapshot::actions::capture(&[], None, Instant::now())?,
    )?;
    finished = clock::ticks();
    Ok(saved)
  })?;
  anyhow::ensure!(request.poll()?.is_none());
  group.arm(1)?;
  group.record(cpu, 0, 1, access)?;
  let checkpoint = request
    .poll()?
    .ok_or_else(|| anyhow::anyhow!("Primary cut was not captured"))?;
  let epoch = checkpoint.cpus[0]
    .clock
    .rebase(0)
    .wrapping_neg()
    .wrapping_add(offset);
  anyhow::ensure!(
    epoch >= finished,
    "Frozen clock preceded blocked primary capture"
  );
  anyhow::ensure!(epoch <= clock::ticks());

  let mut failed = group.request()?;
  group.record_primary(1, || anyhow::bail!("Deliberate primary capture failure"))?;
  group.arm(1)?;
  group.record(cpu, 0, 1, access)?;
  anyhow::ensure!(
    failed.poll().is_err(),
    "Failed primary cut allowed CPU capture"
  );
  println!("Primary capture gates CPU collection; frozen epoch follows the delayed GIC cut and primary failure rejects");
  Ok(())
}
