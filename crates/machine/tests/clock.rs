#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use machine::hypervisor::state::clock::Frozen;

#[test]
fn stored_time_excludes_suspension_and_does_not_depend_on_the_previous_host_epoch() {
  let snapshot = Frozen::freeze(1000, 300);
  let later = snapshot.rebase(50000);
  assert_eq!(later, 49300);
  let rebooted = snapshot.rebase(100);
  assert_eq!(100u64.wrapping_sub(rebooted), 700);
}

#[test]
fn counter_wrap_is_preserved() {
  let wrapped = Frozen::freeze(u64::MAX - 10, u64::MAX - 20).rebase(100);
  assert_eq!(wrapped, 90);
}
