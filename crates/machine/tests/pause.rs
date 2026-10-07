use machine::pause;
use std::{
  sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
  },
  time::Duration,
};

#[test]
fn requests_are_idempotent_but_old_acknowledgements_cannot_satisfy_new_requests() {
  let gate = pause::create();
  let first = pause::request(&gate).unwrap();
  assert_eq!(pause::request(&gate).unwrap(), first);
  assert!(pause::wait(&gate, first, Duration::from_millis(1)).is_err());
  pause::resume(&gate);
  assert!(pause::wait(&gate, first, Duration::ZERO).is_err());
  let second = pause::request(&gate).unwrap();
  assert!(second > first);
  assert!(pause::wait(&gate, first, Duration::ZERO).is_err());
  assert!(pause::wait(&gate, second, Duration::from_millis(1)).is_err());
}

#[test]
fn repeated_generations_hold_the_owner_until_resume_and_stop() {
  let gate = pause::create();
  let stop = Arc::new(AtomicBool::new(false));
  let counter = Arc::new(AtomicU64::new(0));
  std::thread::scope(|scope| {
    let cleanup = pause::stopper(stop.clone(), gate.clone());
    let owner = {
      let gate = gate.clone();
      let stop = stop.clone();
      let counter = counter.clone();
      scope.spawn(move || {
        while pause::checkpoint(&gate, &stop).unwrap() {
          counter.fetch_add(1, Ordering::Relaxed);
        }
      })
    };
    for _ in 0..100 {
      let generation = pause::request(&gate).unwrap();
      pause::wait(&gate, generation, Duration::from_secs(1)).unwrap();
      let held = counter.load(Ordering::Relaxed);
      std::thread::yield_now();
      assert_eq!(counter.load(Ordering::Relaxed), held);
      pause::resume(&gate);
    }
    let generation = pause::request(&gate).unwrap();
    pause::wait(&gate, generation, Duration::from_secs(1)).unwrap();
    drop(cleanup);
    owner.join().unwrap();
  });
}

#[test]
fn resumed_generations_are_rejected_even_if_the_owner_has_not_returned_yet() {
  let gate = pause::create();
  let stop = Arc::new(AtomicBool::new(false));
  std::thread::scope(|scope| {
    let _cleanup = pause::stopper(stop.clone(), gate.clone());
    let generation = pause::request(&gate).unwrap();
    let owner = {
      let gate = gate.clone();
      let stop = stop.clone();
      scope.spawn(move || pause::checkpoint(&gate, &stop).unwrap())
    };
    pause::wait(&gate, generation, Duration::from_secs(1)).unwrap();
    pause::resume(&gate);
    assert!(pause::wait(&gate, generation, Duration::ZERO).is_err());
    owner.join().unwrap();
  });
}
