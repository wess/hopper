use machine::{
  arm::SystemAccess,
  debug,
  psci::{power, Reply},
  snapshot::{actions, wire, Software},
};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

fn fixture() -> anyhow::Result<(Software, actions::State)> {
  let power = power::create(&[0, 1], &[0..0x4000000, 0x40000000..0x50000000])?;
  let mut debug = [debug::State::default(); 2];
  debug::access(
    &mut debug[1],
    SystemAccess {
      encoding: 0x8084,
      register: 0,
      read: false,
    },
    0,
  );
  let now = Instant::now();
  Ok((
    Software::capture(&power, &debug)?,
    actions::capture(
      &[0xa5; 64],
      Some(now + Duration::from_nanos(700123456)),
      now,
    )?,
  ))
}

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let checksum = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&checksum);
}

#[test]
fn reopened_parked_power_debug_and_relative_actions_restore() -> anyhow::Result<()> {
  let (software, actions) = fixture()?;
  let bytes = wire::encode(&software, &actions)?;
  drop((software, actions));
  let saved = wire::decode(&bytes)?;
  assert_eq!(wire::encode(&saved.software, &saved.actions)?, bytes);
  let mut power = power::create(&[0, 1], &[0..0x4000000, 0x40000000..0x50000000])?;
  assert!(matches!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x100, 0]),
    Reply::CpuOn { .. }
  ));
  power::started(&mut power, 1)?;
  let mut debug = [debug::State::default(); 2];
  saved.software.restore(&mut power, &mut debug)?;
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(1)
  );
  assert_eq!(
    debug::access(
      &mut debug[1],
      SystemAccess {
        encoding: 0x808c,
        register: 0,
        read: true
      },
      0
    ),
    Some(8)
  );
  for hold in [Duration::from_secs(2), Duration::from_secs(600)] {
    let now = Instant::now() + hold;
    assert_eq!(
      saved.actions.resume(now)?,
      Some(now + Duration::from_nanos(700123456))
    );
  }
  assert_eq!(saved.actions.tail(), &[0xa5; 64]);
  let empty = actions::capture(&[], None, Instant::now())?;
  let reopened = wire::decode(&wire::encode(&saved.software, &empty)?)?;
  assert!(reopened.actions.resume(Instant::now())?.is_none());
  Ok(())
}

#[test]
fn damage_truncation_and_rechecksummed_invalid_state_reject() -> anyhow::Result<()> {
  let (software, actions) = fixture()?;
  let bytes = wire::encode(&software, &actions)?;
  for end in 0..bytes.len() {
    assert!(wire::decode(&bytes[..end]).is_err());
  }
  for offset in 0..bytes.len() {
    let mut changed = bytes.clone();
    changed[offset] ^= 1;
    assert!(wire::decode(&changed).is_err());
  }
  for (offset, value) in [
    (4, 0),
    (4, 3),
    (8, 1),
    (13, 0),
    (14, 2),
    (23, 2),
    (25, 17),
    (58, 0),
    (67, 65),
  ] {
    let mut changed = bytes.clone();
    changed[offset] = value;
    checksum(&mut changed);
    assert!(
      wire::decode(&changed).is_err(),
      "Accepted invalid field at {offset}"
    );
  }
  for (start, value) in [(15, 0u64), (34, 0), (59, 1_000_000_001)] {
    let mut changed = bytes.clone();
    changed[start..start + 8].copy_from_slice(&value.to_le_bytes());
    checksum(&mut changed);
    assert!(wire::decode(&changed).is_err());
  }
  let mut extended = bytes.to_vec();
  extended.insert(extended.len() - 32, 0);
  checksum(&mut extended);
  assert!(wire::decode(&extended).is_err());
  assert!(wire::decode(&vec![0; wire::MAX + 1]).is_err());
  Ok(())
}

#[test]
fn unsupported_topology_and_foreign_restore_reject() -> anyhow::Result<()> {
  let power = power::create(&[0, 1, 2], std::slice::from_ref(&(0..0x4000000)))?;
  let software = Software::capture(&power, &[debug::State::default(); 3])?;
  let actions = actions::capture(&[], None, Instant::now())?;
  assert!(wire::encode(&software, &actions).is_err());
  let (software, actions) = fixture()?;
  let saved = wire::decode(&wire::encode(&software, &actions)?)?;
  let mut foreign = power::create(&[0, 2], &[0..0x4000000, 0x40000000..0x50000000])?;
  let before = power::snapshot::capture(&foreign)?;
  let mut debug = [debug::State::default(); 2];
  assert!(saved.software.restore(&mut foreign, &mut debug).is_err());
  assert_eq!(power::snapshot::capture(&foreign)?, before);
  assert_eq!(debug, [debug::State::default(); 2]);
  Ok(())
}
