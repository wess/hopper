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

#[test]
fn observer_runs_on_the_held_owner_after_acknowledgement() {
  let gate = pause::create();
  let stop = AtomicBool::new(false);
  let generation = pause::request(&gate).unwrap();
  let (seen, events) = std::sync::mpsc::channel();
  std::thread::scope(|scope| {
    let owner = scope.spawn(|| {
      pause::checkpoint_with(&gate, &stop, |observed| {
        seen.send(observed).unwrap();
        Ok(())
      })
      .unwrap()
    });
    pause::wait(&gate, generation, Duration::from_secs(2)).unwrap();
    assert_eq!(
      events.recv_timeout(Duration::from_secs(2)).unwrap(),
      generation
    );
    assert_eq!(
      events.recv_timeout(Duration::from_secs(2)).unwrap(),
      generation
    );
    assert!(!owner.is_finished());
    pause::resume(&gate);
    assert!(owner.join().unwrap());
  });
}

#[test]
fn resume_during_observation_does_not_acknowledge_the_cancelled_generation() {
  let gate = pause::create();
  let stop = AtomicBool::new(false);
  let generation = pause::request(&gate).unwrap();
  let (seen, events) = std::sync::mpsc::sync_channel(1);
  let (release, released) = std::sync::mpsc::sync_channel(1);
  std::thread::scope(|scope| {
    let observed_gate = gate.clone();
    let observed_stop = &stop;
    let owner = scope.spawn(move || {
      pause::checkpoint_with(&observed_gate, observed_stop, |observed| {
        seen.send(observed).unwrap();
        released.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok(())
      })
      .unwrap()
    });
    assert_eq!(
      events.recv_timeout(Duration::from_secs(2)).unwrap(),
      generation
    );
    pause::resume(&gate);
    assert!(pause::wait(&gate, generation, Duration::ZERO).is_err());
    release.send(()).unwrap();
    assert!(owner.join().unwrap());
  });
}

#[test]
fn pause_wait_drains_a_reply_before_the_owner_can_acknowledge() {
  let gate = pause::create();
  let stop = AtomicBool::new(false);
  let applied = AtomicBool::new(false);
  let (request, requests) = std::sync::mpsc::sync_channel(1);
  let (reply, replies) = std::sync::mpsc::sync_channel(1);
  let (ready, started) = std::sync::mpsc::sync_channel(1);
  std::thread::scope(|scope| {
    let owner_gate = gate.clone();
    let owner_stop = &stop;
    let owner_applied = &applied;
    let owner = scope.spawn(move || {
      request.send(()).unwrap();
      ready.send(()).unwrap();
      replies.recv_timeout(Duration::from_secs(2)).unwrap();
      owner_applied.store(true, Ordering::Release);
      pause::checkpoint(&owner_gate, owner_stop).unwrap()
    });
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let generation = pause::request(&gate).unwrap();
    let result = pause::wait_with::<()>(&gate, generation, Duration::from_secs(2), || {
      if requests.try_recv().is_ok() {
        reply.send(()).unwrap();
      }
      Ok(None)
    })
    .unwrap();
    assert!(result.is_none());
    assert!(applied.load(Ordering::Acquire));
    pause::resume(&gate);
    assert!(owner.join().unwrap());
  });
}

#[test]
fn progress_can_report_guest_shutdown_without_claiming_pause_completion() {
  let gate = pause::create();
  let generation = pause::request(&gate).unwrap();
  assert_eq!(
    pause::wait_with(&gate, generation, Duration::from_secs(2), || Ok(Some(
      "shutdown"
    )))
    .unwrap(),
    Some("shutdown")
  );
  assert!(pause::wait(&gate, generation, Duration::ZERO).is_err());
  pause::resume(&gate);
  let called = AtomicBool::new(false);
  assert!(
    pause::wait_with::<()>(&gate, generation, Duration::from_secs(2), || {
      called.store(true, Ordering::Relaxed);
      Ok(None)
    })
    .is_err()
  );
  assert!(!called.load(Ordering::Relaxed));
}
