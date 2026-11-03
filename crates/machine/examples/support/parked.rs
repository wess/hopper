pub fn run() -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::{
    debug,
    hypervisor::state::group,
    psci::{power, Reply},
    runtime::{self, Boot, Control, Mode},
  };
  let group = group::create(2)?;
  let mut firmware = [
    0xd2a80024u32,
    0xf9400085,
    0xb4ffffe5,
    0x52800060,
    0x72b88000,
    0xd2800021,
    0xd2802002,
    0xd2800003,
    0xd4000002,
    0x91004084,
    0xf9400085,
    0x910004a5,
    0xf9000085,
    0x17fffffd,
  ]
  .into_iter()
  .flat_map(u32::to_le_bytes)
  .collect::<Vec<_>>();
  firmware.resize(0x100, 0);
  firmware.extend(
    [
      0xd2a80024u32,
      0x91002084,
      0xd2a12006,
      0x52806028,
      0xb90030c8,
      0x52800827,
      0xf9400085,
      0x910004a5,
      0xb90000c7,
      0xf9000085,
      0x17fffffc,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes),
  );
  let boot = || Boot {
    firmware: firmware.clone(),
    variables: vec![0xff],
    memory: 0x10000000,
    cpus: 2,
    timeout: std::time::Duration::from_secs(5),
  };
  let mut pending = None;
  let mut captured = None;
  let stopped = runtime::run_captured(
    boot(),
    std::array::from_fn(|_| None),
    group.clone(),
    |_, _, mode| {
      if mode == Mode::Paused {
        if pending.is_none() {
          pending = Some(group.request()?);
        }
        if let Some(checkpoint) = pending.as_mut().context("Missing parked capture")?.poll()? {
          captured = Some(checkpoint);
          return Ok(Control::Stop);
        }
      }
      Ok(Control::Pause)
    },
  )?;
  drop(stopped);
  let captured = super::records::reopen(
    captured.context("Parked CPU capture did not finish")?,
    || Ok(std::array::from_fn(|_| None)),
  )?;
  ensure!(
    captured.cpus.len() == 2
      && captured.primary.software.power().online(0)?
      && !captured.primary.software.power().online(1)?
  );
  let mut power = power::create(&[0, 1], &[0..0x4000000, 0x40000000..0x50000000])?;
  ensure!(matches!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x100, 0]),
    Reply::CpuOn { .. }
  ));
  power::started(&mut power, 1)?;
  let mut debug = [debug::State::default(); 2];
  debug::access(
    &mut debug[1],
    machine::arm::SystemAccess {
      encoding: 0x8084,
      register: 0,
      read: false,
    },
    0,
  );
  captured.primary.software.restore(&mut power, &mut debug)?;
  ensure!(power::call(&mut power, 0, 0xc4000004, [1, 0, 0]) == Reply::Value(1));
  ensure!(
    debug::access(
      &mut debug[1],
      machine::arm::SystemAccess {
        encoding: 0x808c,
        register: 0,
        read: true
      },
      0
    ) == Some(10)
  );
  let mut resumed = false;
  let stopped = runtime::run_restored(
    boot(),
    std::array::from_fn(|_| None),
    &captured,
    |_, ram, mode| {
      let mut counters = [0; 24];
      machine::hypervisor::read(ram, 0x10000, &mut counters)?;
      if !resumed {
        ensure!(mode == Mode::Paused && counters == [0; 24]);
        machine::hypervisor::write(ram, 0x10000, &1u64.to_le_bytes())?;
        resumed = true;
      } else if counters[8..16] != [0; 8] && counters[16..24] != [0; 8] {
        return Ok(Control::Stop);
      }
      Ok(Control::Continue)
    },
  )?;
  ensure!(
    resumed && !stopped.console.is_empty() && stopped.console.iter().all(|&byte| byte == b'A')
  );
  drop(stopped);
  println!("Parked secondary CPU remains off through reconstruction, then guest CPU_ON starts it and both owners execute after recovery");
  println!("Parked secondary CPU capture and software power/debug restoration passed");
  Ok(())
}
