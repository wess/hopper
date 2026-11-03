use anyhow::{ensure, Context};
use machine::{
  hypervisor::{self as hv, state::group},
  runtime::{self, Boot, Control, Mode},
};
use std::time::{Duration, Instant};

const BASE: usize = 0x100000;
const FLAG: usize = 0x180000;
const SETTINGS: [(u16, u64); 7] = [
  (0xdce0, 0x41),
  (0xdce1, 1 << 31),
  (0xdce5, 7),
  (0xdcf0, 15),
  (0xc4f1, 1 << 31),
  (0xdf7f, 0xc8000000),
  (0xdcf3, 1 << 31),
];

fn immediate(code: &mut Vec<u32>, register: u32, value: u64) {
  for half in 0..4 {
    code.push(
      (if half == 0 { 0xd2800000 } else { 0xf2800000 })
        | (half << 21)
        | (((value >> (half * 16)) as u32 & 0xffff) << 5)
        | register,
    );
  }
}

fn system(register: u16, target: u32, write: bool) -> u32 {
  (if write { 0xd5100000 } else { 0xd5300000 }) | (u32::from(register & 0x7fff) << 5) | target
}

fn store(register: u32, offset: u32) -> u32 {
  0xf9000000 | ((offset / 8) << 10) | (20 << 5) | register
}

fn program(index: u64) -> Vec<u32> {
  let mut code = Vec::new();
  if index == 0 {
    for (register, value) in [(0, 0xc4000003), (1, 1), (2, 0x200), (3, 0)] {
      immediate(&mut code, register, value);
    }
    code.push(0xd4000002);
  }
  immediate(&mut code, 20, 0x40000000 + BASE as u64 + index * 0x1000);
  immediate(&mut code, 21, 0x40000000 + FLAG as u64);
  immediate(&mut code, 22, 0);
  for (register, value) in SETTINGS {
    immediate(
      &mut code,
      0,
      if register == 0xdce5 {
        value + index * 4
      } else {
        value
      },
    );
    code.push(system(register, 0, true));
  }
  immediate(
    &mut code,
    0,
    if index == 0 {
      0x1234000000000000
    } else {
      0x5678000000000000
    },
  );
  code.push(system(0xdce8, 0, true));
  code.extend([system(0xdf00, 4, false), 0xd341fc84]);
  for (counter, compare) in [(0xdf01, 0xdf12), (0xdf02, 0xdf1a)] {
    code.extend([
      system(counter, 0, false),
      0x8b040000,
      system(compare, 0, true),
    ]);
  }
  immediate(&mut code, 0, 1);
  code.extend([
    system(0xdf11, 0, true),
    system(0xdf19, 0, true),
    store(0, 0),
  ]);
  let wait = code.len();
  code.extend([system(0xdce8, 0, false), store(0, 8), 0xf94002a1]);
  let branch = wait as i32 - code.len() as i32;
  code.push(0xb4000001 | ((branch as u32 & 0x7ffff) << 5));
  for (slot, (register, _)) in SETTINGS.into_iter().enumerate() {
    code.extend([system(register, 0, false), store(0, 16 + slot as u32 * 8)]);
  }
  let progress = code.len();
  for (register, offset) in [(0xdce8, 80), (0xdf11, 88), (0xdf19, 96)] {
    code.extend([system(register, 0, false), store(0, offset)]);
  }
  code.extend([0x910006d6, store(22, 120)]);
  let branch = progress as i32 - code.len() as i32;
  code.push(0x14000000 | (branch as u32 & 0x3ffffff));
  code
}

fn boot() -> Boot {
  let mut firmware = vec![0; 4096];
  for index in 0..2 {
    let bytes = program(index)
      .into_iter()
      .flat_map(u32::to_le_bytes)
      .collect::<Vec<_>>();
    assert!(bytes.len() <= 512);
    let offset = index as usize * 512;
    firmware[offset..offset + bytes.len()].copy_from_slice(&bytes);
  }
  Boot {
    firmware,
    variables: vec![0xff; 4096],
    memory: 0x10000000,
    cpus: 2,
    timeout: Duration::from_secs(6),
  }
}

fn read(ram: &hv::Memory<'_>, cpu: usize, offset: usize) -> anyhow::Result<u64> {
  let mut value = [0; 8];
  hv::read(ram, BASE + cpu * 0x1000 + offset, &mut value)?;
  Ok(u64::from_le_bytes(value))
}

pub fn run() -> anyhow::Result<()> {
  let group = group::create(2)?;
  let mut request = None;
  let mut checkpoint = None;
  let stopped = runtime::run_captured(
    boot(),
    std::array::from_fn(|_| None),
    group.clone(),
    |_, ram, mode| {
      if mode == Mode::Running {
        return Ok(if read(ram, 0, 0)? == 1 && read(ram, 1, 0)? == 1 {
          Control::Pause
        } else {
          Control::Continue
        });
      }
      if request.is_none() {
        request = Some(group.request()?);
      } else if let Some(saved) = request.as_mut().context("Missing PMU request")?.poll()? {
        checkpoint = Some(saved);
        return Ok(Control::Stop);
      }
      Ok(Control::Pause)
    },
  )?;
  drop(stopped);
  let checkpoint = checkpoint.context("PMU runtime checkpoint was not captured")?;
  let checkpoint = super::records::reopen(checkpoint, || Ok(std::array::from_fn(|_| None)))?;
  let mut baseline = [0; 2];
  for (cpu, value) in baseline.iter_mut().enumerate() {
    let mut bytes = [0; 8];
    checkpoint
      .primary
      .memory
      .ram
      .read(BASE + cpu * 0x1000 + 8, &mut bytes)?;
    *value = u64::from_le_bytes(bytes);
  }
  for attempt in 0..2 {
    recover(&checkpoint, baseline, attempt)?;
    for (cpu, expected) in baseline.into_iter().enumerate() {
      let mut bytes = [0; 8];
      checkpoint
        .primary
        .memory
        .ram
        .read(BASE + cpu * 0x1000 + 8, &mut bytes)?;
      ensure!(
        u64::from_le_bytes(bytes) == expected,
        "Recovery changed captured source RAM"
      );
    }
  }
  println!("Two-CPU PMU runtime recovery: independent settings/cycle epochs, 750 ms initial hold, enabled timer delay/expiry and repeated restoration passed");
  Ok(())
}

fn recover(saved: &group::Checkpoint, baseline: [u64; 2], attempt: usize) -> anyhow::Result<()> {
  let mut held = None;
  let mut continued = false;
  let mut active = None;
  let mut progress = [0; 2];
  let mut complete = false;
  let stopped = runtime::run_restored(
    boot(),
    std::array::from_fn(|_| None),
    saved,
    |_, ram, mode| {
      if mode == Mode::Paused {
        let since = *held.get_or_insert_with(Instant::now);
        return Ok(if since.elapsed() >= Duration::from_millis(750) {
          Control::Continue
        } else {
          Control::Pause
        });
      }
      if !continued {
        hv::write(ram, FLAG, &1u64.to_le_bytes())?;
        continued = true;
      }
      if read(ram, 0, 120)? == 0 || read(ram, 1, 120)? == 0 {
        return Ok(Control::Continue);
      }
      if active.is_none() {
        for cpu in 0..2 {
          for (slot, (register, value)) in SETTINGS.into_iter().enumerate() {
            let expected = if register == 0xdce5 {
              value + cpu as u64 * 4
            } else {
              value
            };
            ensure!(
              read(ram, cpu, 16 + slot * 8)? == expected,
              "CPU {cpu} PMU setting 0x{register:04x} was not recovered"
            );
          }
          let cycle = read(ram, cpu, 80)?;
          ensure!(
            cycle >= baseline[cpu] && cycle - baseline[cpu] < 1 << 30,
            "CPU {cpu} PMU included the initial hold"
          );
          ensure!(
            read(ram, cpu, 88)? & 5 == 1 && read(ram, cpu, 96)? & 5 == 1,
            "CPU {cpu} timers aged during the initial hold"
          );
          progress[cpu] = read(ram, cpu, 120)?;
        }
        active = Some(Instant::now());
      }
      if active.is_some_and(|since| since.elapsed() >= Duration::from_millis(550)) {
        for (cpu, previous) in progress.into_iter().enumerate() {
          ensure!(
            read(ram, cpu, 88)? & 5 == 5 && read(ram, cpu, 96)? & 5 == 5,
            "CPU {cpu} timers failed to expire after recovery"
          );
          ensure!(read(ram, cpu, 120)? > previous);
        }
        complete = true;
        return Ok(Control::Stop);
      }
      Ok(Control::Continue)
    },
  )?;
  drop(stopped);
  ensure!(complete, "PMU recovery attempt {attempt} did not complete");
  println!("PMU recovery attempt {attempt}: both CPU settings/cycles and held-then-expiring timers verified");
  Ok(())
}
