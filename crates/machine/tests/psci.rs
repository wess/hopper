use machine::psci::{call, Reply};

#[test]
fn feature_queries_do_not_advertise_secondary_cpu_start() {
  assert_eq!(call(0x84000000, 0, 0), Reply::Value(0x10000));
  assert_eq!(call(0x8400000a, 0x84000008, 0), Reply::Value(0));
  assert_eq!(call(0x8400000a, 0xc4000003, 0), Reply::Value(-1));
  assert_eq!(call(0xc4000003, 1, 0), Reply::Value(-1));
}

#[test]
fn affinity_queries_check_target_and_level() {
  assert_eq!(call(0xc4000004, 0, 0), Reply::Value(0));
  assert_eq!(call(0xc4000004, 1, 0), Reply::Value(-2));
  assert_eq!(call(0xc4000004, 0x100000000, 0), Reply::Value(-2));
  assert_eq!(call(0xc4000004, 0, 4), Reply::Value(-2));
}

#[test]
fn power_requests_are_explicit_runtime_actions() {
  assert_eq!(call(0x84000002, 0, 0), Reply::CpuOff);
  assert_eq!(call(0x84000008, 0, 0), Reply::Shutdown);
  assert_eq!(call(0x84000009, 0, 0), Reply::Reset);
}
