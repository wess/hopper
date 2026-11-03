use machine::snapshot::actions;
use std::time::{Duration, Instant};

#[test]
fn action_deadlines_exclude_recovery_hold_and_support_reusable_restoration() {
  let now = Instant::now();
  let saved = actions::capture(b"cdboot.efi", Some(now + Duration::from_millis(700)), now).unwrap();
  assert_eq!(saved.tail(), b"cdboot.efi");
  for delay in [Duration::from_secs(2), Duration::from_secs(600)] {
    let resumed = now + delay;
    assert_eq!(
      saved.resume(resumed).unwrap(),
      Some(resumed + Duration::from_millis(700))
    );
  }
  assert!(actions::capture(&[0; 65], None, now).is_err());
  assert!(actions::capture(&[], Some(now + Duration::from_secs(2)), now).is_err());
}

#[test]
fn absent_and_overdue_actions_preserve_bounded_tails_without_old_deadlines() {
  let now = Instant::now();
  let empty = actions::capture(&[0; 64], None, now).unwrap();
  assert_eq!(empty.tail(), &[0; 64]);
  assert!(empty
    .resume(now + Duration::from_secs(5))
    .unwrap()
    .is_none());
  let overdue = actions::capture(b"tail", Some(now), now + Duration::from_secs(2)).unwrap();
  let resumed = now + Duration::from_secs(20);
  assert_eq!(overdue.resume(resumed).unwrap(), Some(resumed));
}
