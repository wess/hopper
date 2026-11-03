use super::{spawn, Check, Publication};
use crate::snapshot::{
  manifest::{Binding, Layout},
  package::{Package, Sealed, Stage},
};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, Permissions},
  os::unix::fs::PermissionsExt,
  sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};

struct Owner(Arc<AtomicUsize>);
impl Drop for Owner {
  fn drop(&mut self) {
    self.0.fetch_add(1, Ordering::AcqRel);
  }
}

fn binding() -> Binding {
  Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: [3; 32],
  }
}

fn layout() -> Layout {
  Layout {
    cpus: 1,
    banks: [(0, 65536), (65536, 65536), (1 << 30, 65536)],
    devices: [0; 7],
  }
}

fn job(
  root: &std::path::Path,
  drops: Arc<AtomicUsize>,
  check: Check,
) -> anyhow::Result<Publication<Owner, Sealed>> {
  std::fs::set_permissions(root, Permissions::from_mode(0o700))?;
  let parent = File::open(root)?;
  spawn(Owner(drops), Duration::from_secs(2), check, move |check| {
    let mut stage = Stage::new(&parent, binding(), layout(), check)?;
    let mut bytes = b"test component".to_vec();
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    for record in layout().records()? {
      stage.write(record, check, |out| Ok(out.write_all(&bytes)?))?;
    }
    stage.seal(check)
  })
}

fn collect(job: &mut Publication<Owner, Sealed>) -> anyhow::Result<Sealed> {
  let deadline = Instant::now() + Duration::from_secs(1);
  loop {
    if let Some(sealed) = job.poll()? {
      return Ok(sealed);
    }
    anyhow::ensure!(Instant::now() < deadline, "Staging did not finish");
    std::thread::sleep(Duration::from_millis(1));
  }
}

#[test]
fn background_staging_requires_separate_commit_and_retains_owner_through_it() -> anyhow::Result<()>
{
  let root = tempfile::tempdir()?;
  let drops = Arc::new(AtomicUsize::new(0));
  let mut job = job(root.path(), drops.clone(), Arc::new(|| Ok(())))?;
  let sealed = collect(&mut job)?;
  assert_eq!(drops.load(Ordering::Acquire), 0);
  let parent = File::open(root.path())?;
  assert!(Package::open(&parent, binding(), layout(), || Ok(())).is_err());
  let receipt = sealed.receipt();
  drop(sealed);
  let reopened = Sealed::open(&parent, receipt, binding(), layout(), || job.check())?;
  drop(reopened.publish(|| job.check(), || Ok(()))?);
  assert!(Package::open(&parent, binding(), layout(), || job.check()).is_ok());
  assert!(job.poll().is_err());
  assert_eq!(drops.load(Ordering::Acquire), 0);
  drop(job);
  assert_eq!(drops.load(Ordering::Acquire), 1);
  Ok(())
}

#[test]
fn cancellation_after_collecting_a_stage_rejects_owner_commit() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  let drops = Arc::new(AtomicUsize::new(0));
  let mut job = job(root.path(), drops.clone(), Arc::new(|| Ok(())))?;
  let sealed = collect(&mut job)?;
  job.cancel();
  assert!(sealed.publish(|| job.check(), || Ok(())).is_err());
  assert_eq!(drops.load(Ordering::Acquire), 0);
  assert!(Package::open(&File::open(root.path())?, binding(), layout(), || Ok(())).is_err());
  assert_eq!(std::fs::read_dir(root.path())?.count(), 1);
  drop(job);
  assert_eq!(drops.load(Ordering::Acquire), 1);
  Ok(())
}

#[test]
fn cached_stage_cannot_be_collected_after_revocation_or_revived() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  let drops = Arc::new(AtomicUsize::new(0));
  let allowed = Arc::new(AtomicBool::new(true));
  let policy = allowed.clone();
  let mut job = job(
    root.path(),
    drops.clone(),
    Arc::new(move || {
      anyhow::ensure!(policy.load(Ordering::Acquire), "revoked");
      Ok(())
    }),
  )?;
  let deadline = Instant::now() + Duration::from_secs(1);
  while !job.worker.as_ref().unwrap().is_finished() {
    anyhow::ensure!(Instant::now() < deadline);
    std::thread::sleep(Duration::from_millis(1));
  }
  allowed.store(false, Ordering::Release);
  assert!(job.poll().is_err());
  allowed.store(true, Ordering::Release);
  assert!(job.poll().is_err());
  assert!(job.check().is_err());
  assert!(Package::open(&File::open(root.path())?, binding(), layout(), || Ok(())).is_err());
  drop(job);
  assert_eq!(drops.load(Ordering::Acquire), 1);
  assert_eq!(std::fs::read_dir(root.path())?.count(), 1);
  Ok(())
}
