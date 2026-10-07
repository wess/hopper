use machine::runtime::{self, Boot};
use std::time::Duration;

fn boot() -> Boot {
  Boot {
    firmware: vec![0],
    variables: vec![0],
    memory: 0x100000000,
    cpus: 2,
    timeout: Duration::from_secs(60),
  }
}

#[test]
fn invalid_hardware_is_rejected_before_native_allocation() {
  let mut config = boot();
  runtime::validate(&config).unwrap();
  for memory in [0, 0x10000000 - 1, 0x10000001, 0x1000000001, u64::MAX] {
    config.memory = memory;
    assert!(runtime::validate(&config).is_err());
  }
  for memory in [0x10000000, 0x1000000000] {
    config.memory = memory;
    runtime::validate(&config).unwrap();
  }
  for cpus in [0, 3, u32::MAX] {
    config.cpus = cpus;
    assert!(runtime::validate(&config).is_err());
  }
  config.cpus = 1;
  for timeout in [Duration::ZERO, Duration::from_millis(19)] {
    config.timeout = timeout;
    assert!(runtime::validate(&config).is_err());
  }
  config.timeout = Duration::MAX;
  runtime::validate(&config).unwrap();
}

#[test]
fn empty_and_oversized_firmware_or_variables_are_rejected() {
  let mut config = boot();
  config.firmware.clear();
  assert!(runtime::validate(&config).is_err());
  config.firmware = vec![0; 0x4000001];
  assert!(runtime::validate(&config).is_err());
  config.firmware = vec![0];
  config.variables.clear();
  assert!(runtime::validate(&config).is_err());
  config.variables = vec![0; 0x4000001];
  assert!(runtime::validate(&config).is_err());
}
