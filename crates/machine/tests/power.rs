use machine::psci::{power, Reply};

fn create() -> power::Power {
  power::create(
    &[0, 1, 0x100],
    std::slice::from_ref(&(0x40000000..0x50000000)),
  )
  .unwrap()
}

#[test]
fn starts_preserve_context_and_distinguish_pending_on_and_off() {
  let mut power = create();
  for command in [0x84000003, 0xc4000003] {
    assert_eq!(
      power::call(&mut power, 0, 0x8400000a, [command, 0, 0]),
      Reply::Value(0)
    );
  }
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(1)
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x40001000, u64::MAX]),
    Reply::CpuOn {
      target: 1,
      entry: 0x40001000,
      context: u64::MAX
    }
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(2)
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x40001000, 0]),
    Reply::Value(-5)
  );
  power::started(&mut power, 1).unwrap();
  assert_eq!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x40001000, 0]),
    Reply::Value(-4)
  );
  assert_eq!(
    power::call(&mut power, 1, 0x84000002, [0; 3]),
    Reply::CpuOff
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(0)
  );
  power::stopped(&mut power, 1).unwrap();
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
    Reply::Value(1)
  );
}

#[test]
fn invalid_requests_leave_secondary_off_and_failed_starts_can_retry() {
  let mut power = create();
  for (target, entry, status) in [
    (2, 0x40001000, -2),
    (1, 0x40001001, -9),
    (1, 0x50000000, -9),
    (1, u64::MAX - 3, -9),
  ] {
    assert_eq!(
      power::call(&mut power, 0, 0xc4000003, [target, entry, 0]),
      Reply::Value(status)
    );
    assert_eq!(
      power::call(&mut power, 0, 0xc4000004, [1, 0, 0]),
      Reply::Value(1)
    );
  }
  assert!(power::started(&mut power, 1).is_err());
  assert!(power::stopped(&mut power, 1).is_err());
  assert!(power::failed(&mut power, 1).is_err());
  assert!(matches!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x40001000, 0]),
    Reply::CpuOn { .. }
  ));
  power::failed(&mut power, 1).unwrap();
  assert!(matches!(
    power::call(&mut power, 0, 0xc4000003, [1, 0x40001000, 42]),
    Reply::CpuOn { context: 42, .. }
  ));
}

#[test]
fn affinity_groups_include_all_cores_and_smc32_truncates_arguments() {
  let mut power = create();
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [0x100, 1, 0]),
    Reply::Value(1)
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [0, 1, 0]),
    Reply::Value(0)
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [0x10000, 2, 0]),
    Reply::Value(-2)
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [0, 4, 0]),
    Reply::Value(-2)
  );
  assert_eq!(
    power::call(
      &mut power,
      0,
      0x84000003,
      [0x100000001, 0x140001000, 0x10000002a]
    ),
    Reply::CpuOn {
      target: 1,
      entry: 0x40001000,
      context: 42
    }
  );
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [1, 1, 0]),
    Reply::Value(0)
  );
  assert!(matches!(
    power::call(&mut power, 0, 0xc4000003, [0x100, 0x40001000, 0]),
    Reply::CpuOn { target: 2, .. }
  ));
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [0x100, 1, 0]),
    Reply::Value(2)
  );
  power::started(&mut power, 2).unwrap();
  assert_eq!(
    power::call(&mut power, 0, 0xc4000004, [0x100, 1, 0]),
    Reply::Value(0)
  );
  power::stopped(&mut power, 2).unwrap();
  assert_eq!(
    power::call(&mut power, 2, 0x84000000, [0; 3]),
    Reply::Value(-3)
  );
}

#[test]
fn topology_rejects_ambiguous_affinities_and_invalid_entry_regions() {
  assert!(power::create(&[], std::slice::from_ref(&(0..4))).is_err());
  assert!(power::create(&[0, 0x80000000], std::slice::from_ref(&(0..4))).is_err());
  assert!(power::create(&[0], &[]).is_err());
  assert!(power::create(&[0], std::slice::from_ref(&(4..4))).is_err());
}
