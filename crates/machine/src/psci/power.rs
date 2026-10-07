use super::Reply;
use anyhow::ensure;
use std::ops::Range;

const AFFINITY: u64 = 0xff00ffffff;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
  On,
  Off,
  Pending,
}

pub struct Power {
  cores: Vec<(u64, State)>,
  executable: Vec<Range<u64>>,
}

pub fn create(affinities: &[u64], executable: &[Range<u64>]) -> anyhow::Result<Power> {
  ensure!(
    !affinities.is_empty() && affinities.len() <= 256,
    "Invalid CPU topology"
  );
  let mut cores = Vec::new();
  for affinity in affinities {
    let affinity = affinity & AFFINITY;
    ensure!(
      !cores.iter().any(|(other, _)| *other == affinity),
      "Duplicate CPU affinity"
    );
    cores.push((affinity, State::Off));
  }
  ensure!(
    !executable.is_empty() && executable.iter().all(|range| range.start < range.end),
    "Invalid CPU entry regions"
  );
  cores[0].1 = State::On;
  Ok(Power {
    cores,
    executable: executable.to_vec(),
  })
}

/// Commit startup before the CPU executes its first guest instruction.
pub fn started(power: &mut Power, target: usize) -> anyhow::Result<()> {
  let core = power
    .cores
    .get_mut(target)
    .ok_or_else(|| anyhow::anyhow!("Unknown CPU"))?;
  ensure!(core.1 == State::Pending, "CPU has no pending start");
  core.1 = State::On;
  Ok(())
}

/// Commit CPU_OFF only after its owner stops execution; the request alone leaves it on.
pub fn stopped(power: &mut Power, target: usize) -> anyhow::Result<()> {
  let core = power
    .cores
    .get_mut(target)
    .ok_or_else(|| anyhow::anyhow!("Unknown CPU"))?;
  ensure!(core.1 == State::On, "CPU is not on");
  core.1 = State::Off;
  Ok(())
}

/// Roll back a startup rejected by the CPU owner before guest execution begins.
pub fn failed(power: &mut Power, target: usize) -> anyhow::Result<()> {
  let core = power
    .cores
    .get_mut(target)
    .ok_or_else(|| anyhow::anyhow!("Unknown CPU"))?;
  ensure!(core.1 == State::Pending, "CPU has no pending start");
  core.1 = State::Off;
  Ok(())
}

fn info(power: &Power, target: u64, level: u64) -> i64 {
  if level > 3 {
    return -2;
  }
  let mask = AFFINITY
    & match level {
      0 => u64::MAX,
      1 => !0xff,
      2 => !0xffff,
      _ => !0xffffff,
    };
  let mut found = false;
  let mut pending = false;
  for (affinity, state) in &power.cores {
    if affinity & mask == target & mask {
      found = true;
      if *state == State::On {
        return 0;
      }
      pending |= *state == State::Pending;
    }
  }
  if !found {
    -2
  } else if pending {
    2
  } else {
    1
  }
}

pub fn call(power: &mut Power, caller: usize, command: u32, mut args: [u64; 3]) -> Reply {
  if power
    .cores
    .get(caller)
    .is_none_or(|(_, state)| *state != State::On)
  {
    return Reply::Value(-3);
  }
  if command & (1 << 30) == 0 {
    args = args.map(|value| value as u32 as u64);
  }
  match command {
    0x84000003 | 0xc4000003 => {
      let Some(target) = power
        .cores
        .iter()
        .position(|(affinity, _)| *affinity == args[0] & AFFINITY)
      else {
        return Reply::Value(-2);
      };
      if !args[1].is_multiple_of(4)
        || !power.executable.iter().any(|range| {
          args[1] >= range.start && args[1].checked_add(4).is_some_and(|end| end <= range.end)
        })
      {
        return Reply::Value(-9);
      }
      match power.cores[target].1 {
        State::On => Reply::Value(-4),
        State::Pending => Reply::Value(-5),
        State::Off => {
          power.cores[target].1 = State::Pending;
          Reply::CpuOn {
            target,
            entry: args[1],
            context: args[2],
          }
        }
      }
    }
    0x84000004 | 0xc4000004 => Reply::Value(info(power, args[0], args[1])),
    0x8400000a if matches!(args[0] as u32, 0x84000003 | 0xc4000003) => Reply::Value(0),
    _ => super::call(command, args[0], args[1] as u32),
  }
}
