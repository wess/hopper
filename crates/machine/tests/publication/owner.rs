use super::{spawn, Check};
use std::{
  sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc,
  },
  time::{Duration, Instant},
};

struct Owner(Arc<AtomicUsize>);
impl Drop for Owner {
  fn drop(&mut self) {
    self.0.fetch_add(1, Ordering::AcqRel);
  }
}
struct Finished(Arc<AtomicBool>);
impl Drop for Finished {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

fn allow() -> Check {
  Arc::new(|| Ok(()))
}

#[test]
fn rejected_admission_never_starts_work_and_releases_ownership() {
  let drops = Arc::new(AtomicUsize::new(0));
  let started = Arc::new(AtomicBool::new(false));
  let observed = started.clone();
  let result = spawn::<_, super::Package>(
    Owner(drops.clone()),
    Duration::from_secs(1),
    Arc::new(|| anyhow::bail!("revoked")),
    move |_| {
      observed.store(true, Ordering::Release);
      anyhow::bail!("must not execute")
    },
  );
  assert!(result.err().unwrap().to_string().contains("revoked"));
  assert!(!started.load(Ordering::Acquire));
  assert_eq!(drops.load(Ordering::Acquire), 1);
}

#[test]
fn revocation_is_permanent_and_teardown_waits_for_the_writer() -> anyhow::Result<()> {
  let allowed = Arc::new(AtomicBool::new(true));
  let policy = allowed.clone();
  let check: Check = Arc::new(move || {
    anyhow::ensure!(policy.load(Ordering::Acquire), "revoked");
    Ok(())
  });
  let drops = Arc::new(AtomicUsize::new(0));
  let finished = Arc::new(AtomicBool::new(false));
  let done = finished.clone();
  let (entered, ready) = mpsc::sync_channel(1);
  let (release, released) = mpsc::sync_channel(1);
  let mut job = spawn::<_, super::Package>(
    Owner(drops.clone()),
    Duration::from_secs(2),
    check,
    move |check| {
      let _finished = Finished(done);
      entered.send(())?;
      released.recv_timeout(Duration::from_secs(1))?;
      check()?;
      anyhow::bail!("must not publish")
    },
  )?;
  ready.recv_timeout(Duration::from_secs(1))?;
  assert!(job.poll()?.is_none());
  allowed.store(false, Ordering::Release);
  assert!(job.poll().err().unwrap().to_string().contains("revoked"));
  allowed.store(true, Ordering::Release);
  assert!(job.poll().err().unwrap().to_string().contains("cancelled"));
  assert_eq!(drops.load(Ordering::Acquire), 0);
  release.send(())?;
  drop(job);
  assert!(finished.load(Ordering::Acquire));
  assert_eq!(drops.load(Ordering::Acquire), 1);
  Ok(())
}

#[test]
fn dropping_a_running_job_cancels_and_joins_before_releasing_ownership() -> anyhow::Result<()> {
  let drops = Arc::new(AtomicUsize::new(0));
  let finished = Arc::new(AtomicBool::new(false));
  let done = finished.clone();
  let (entered, ready) = mpsc::sync_channel(1);
  let job = spawn::<_, super::Package>(
    Owner(drops.clone()),
    Duration::from_secs(2),
    allow(),
    move |check| {
      let _finished = Finished(done);
      entered.send(())?;
      loop {
        check()?;
        std::thread::sleep(Duration::from_millis(1));
      }
    },
  )?;
  ready.recv_timeout(Duration::from_secs(1))?;
  drop(job);
  assert!(finished.load(Ordering::Acquire));
  assert_eq!(drops.load(Ordering::Acquire), 1);
  Ok(())
}

#[test]
fn completed_results_still_require_original_policy_and_collect_only_once() -> anyhow::Result<()> {
  let allowed = Arc::new(AtomicBool::new(true));
  let policy = allowed.clone();
  let mut job = spawn::<_, super::Package>(
    (),
    Duration::from_secs(2),
    Arc::new(move || {
      anyhow::ensure!(policy.load(Ordering::Acquire), "revoked");
      Ok(())
    }),
    |_| anyhow::bail!("writer failure"),
  )?;
  let deadline = Instant::now() + Duration::from_secs(1);
  while !job.worker.as_ref().unwrap().is_finished() {
    anyhow::ensure!(Instant::now() < deadline);
    std::thread::sleep(Duration::from_millis(1));
  }
  allowed.store(false, Ordering::Release);
  assert!(job.poll().err().unwrap().to_string().contains("revoked"));
  allowed.store(true, Ordering::Release);
  assert!(job.poll().err().unwrap().to_string().contains("cancelled"));
  drop(job);
  let mut job = spawn::<_, super::Package>((), Duration::from_secs(2), allow(), |_| {
    anyhow::bail!("writer failure")
  })?;
  let deadline = Instant::now() + Duration::from_secs(1);
  while !job.worker.as_ref().unwrap().is_finished() {
    anyhow::ensure!(Instant::now() < deadline);
    std::thread::sleep(Duration::from_millis(1));
  }
  assert!(job
    .poll()
    .err()
    .unwrap()
    .to_string()
    .contains("writer failure"));
  assert!(job
    .poll()
    .err()
    .unwrap()
    .to_string()
    .contains("already collected"));
  Ok(())
}

#[test]
fn expired_budget_cannot_deliver_a_completed_result() -> anyhow::Result<()> {
  let mut job = spawn::<_, super::Package>((), Duration::from_millis(10), allow(), |_| {
    anyhow::bail!("writer failure")
  })?;
  std::thread::sleep(Duration::from_millis(20));
  assert!(job
    .poll()
    .err()
    .unwrap()
    .to_string()
    .contains("deadline expired"));
  assert!(job.cancelled.load(Ordering::Acquire));
  Ok(())
}

#[test]
fn explicit_cancel_and_worker_panic_preserve_ownership_until_join() -> anyhow::Result<()> {
  let drops = Arc::new(AtomicUsize::new(0));
  let (entered, ready) = mpsc::sync_channel(1);
  let mut job = spawn::<_, super::Package>(
    Owner(drops.clone()),
    Duration::from_secs(2),
    allow(),
    move |check| {
      entered.send(())?;
      loop {
        check()?;
        std::thread::sleep(Duration::from_millis(1));
      }
    },
  )?;
  ready.recv_timeout(Duration::from_secs(1))?;
  job.cancel();
  assert!(job.poll().err().unwrap().to_string().contains("cancelled"));
  assert_eq!(drops.load(Ordering::Acquire), 0);
  drop(job);
  assert_eq!(drops.load(Ordering::Acquire), 1);
  let mut job = spawn::<_, super::Package>(
    Owner(drops.clone()),
    Duration::from_secs(2),
    allow(),
    |_| panic!("synthetic writer panic"),
  )?;
  let deadline = Instant::now() + Duration::from_secs(1);
  while !job.worker.as_ref().unwrap().is_finished() {
    anyhow::ensure!(Instant::now() < deadline);
    std::thread::sleep(Duration::from_millis(1));
  }
  assert!(job
    .poll()
    .err()
    .unwrap()
    .to_string()
    .contains("writer panicked"));
  assert_eq!(drops.load(Ordering::Acquire), 1);
  drop(job);
  assert_eq!(drops.load(Ordering::Acquire), 2);
  Ok(())
}
