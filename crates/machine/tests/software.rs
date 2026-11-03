use machine::{
  arm::SystemAccess,
  debug,
  psci::{power, Reply},
  snapshot::Software,
};

fn power() -> power::Power {
  power::create(&[0, 1], std::slice::from_ref(&(0..0x4000000))).unwrap()
}

fn lock_status(state: &mut debug::State) -> Option<u64> {
  debug::access(
    state,
    SystemAccess {
      encoding: 0x808c,
      register: 0,
      read: true,
    },
    0,
  )
}

#[test]
fn software_restore_preserves_online_parked_and_debug_lock_state() {
  let mut power = power();
  let mut debug = [debug::State::default(); 2];
  let parked = Software::capture(&power, &debug).unwrap();
  assert!(!parked.power().online(1).unwrap());
  assert!(matches!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x100, 42]),
    Reply::CpuOn { .. }
  ));
  assert!(Software::capture(&power, &debug).is_err());
  power::started(&mut power, 1).unwrap();
  debug::access(
    &mut debug[1],
    SystemAccess {
      encoding: 0x8084,
      register: 0,
      read: false,
    },
    0,
  );
  let online = Software::capture(&power, &debug).unwrap();
  power::stopped(&mut power, 1).unwrap();
  debug[1] = debug::State::default();
  online.restore(&mut power, &mut debug).unwrap();
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(0)
  );
  assert_eq!(lock_status(&mut debug[1]), Some(8));
  parked.restore(&mut power, &mut debug).unwrap();
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(1)
  );
  assert_eq!(lock_status(&mut debug[1]), Some(10));
  assert!(matches!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x100, 42]),
    Reply::CpuOn { .. }
  ));
  assert!(online.restore(&mut power, &mut debug).is_err());
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(2)
  );
  assert_eq!(lock_status(&mut debug[1]), Some(10));
}

#[test]
fn mismatched_topology_ranges_and_debug_counts_reject_before_mutation() {
  let source = power();
  let snapshot = Software::capture(&source, &[debug::State::default(); 2]).unwrap();
  for (affinities, range) in [([0, 2], 0..0x4000000), ([0, 1], 0x100..0x4000000)] {
    let mut target = power::create(&affinities, std::slice::from_ref(&range)).unwrap();
    let before = power::snapshot::capture(&target).unwrap();
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
    assert!(snapshot.restore(&mut target, &mut debug).is_err());
    assert_eq!(power::snapshot::capture(&target).unwrap(), before);
    assert_eq!(lock_status(&mut debug[1]), Some(8));
  }
  let mut target = power();
  let before = power::snapshot::capture(&target).unwrap();
  assert!(snapshot
    .restore(&mut target, &mut [debug::State::default(); 1])
    .is_err());
  assert_eq!(power::snapshot::capture(&target).unwrap(), before);
  assert!(Software::capture(&target, &[debug::State::default(); 1]).is_err());
  assert!(snapshot.debug(2).is_err());
  assert!(snapshot.power().online(2).is_err());
}

#[test]
fn snapshots_reject_a_stopped_primary() {
  let mut power = power();
  power::stopped(&mut power, 0).unwrap();
  assert!(power::snapshot::capture(&power).is_err());
}
