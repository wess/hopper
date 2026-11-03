mod recovery;
mod stage;
use super::{fs, Owner, Record};
use crate::machines::native::snapshot::Policy;
use anyhow::{ensure, Context};
use model::native::{Boot, Restore};
pub use recovery::{recover_preparation, Recovered, Recovery};
use std::{
  fs::{File, OpenOptions},
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  path::PathBuf,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};

pub struct Prepared {
  pub owner: Owner,
  pub boot: Boot,
  pub restore: Restore,
}

pub struct Preparation {
  pub owner: Owner,
  pub result: anyhow::Result<(Boot, Restore)>,
}

struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
  fn drop(&mut self) {
    self.0.store(true, Ordering::Release);
  }
}

pub(super) struct Scope {
  root: File,
  path: PathBuf,
  policy: Policy,
  cancelled: Arc<AtomicBool>,
  continuation: Option<crate::machines::native::Check>,
}

impl Scope {
  fn check(&self) -> anyhow::Result<()> {
    ensure!(
      !self.cancelled.load(Ordering::Acquire),
      "Restore preparation cancelled"
    );
    if let Some(check) = &self.continuation {
      check()?;
    }
    self.policy.check()?;
    let current = OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
      .open(&self.path)?;
    fs::private(&current)?;
    let expected = self.root.metadata()?;
    let observed = current.metadata()?;
    ensure!(
      (expected.dev(), expected.ino()) == (observed.dev(), observed.ino()),
      "Restore preparation namespace changed"
    );
    Ok(())
  }
}

/// Trusted stopped-runtime preparation. The caller supplies a separately captured recoverable prior state.
/// Host paths and prior-state selection must never be accepted directly from an agent endpoint.
pub async fn prepare(
  owner: Owner,
  boot: Boot,
  snapshot: &str,
  rollback: &str,
) -> anyhow::Result<Prepared> {
  prepare_using(owner, boot, snapshot, rollback, None).await
}

/// Trusted recovery preparation into fresh resources; the exact predecessor remains preserved.
/// Callers must separately establish which checkpoint represents the recoverable prior state.
pub async fn prepare_successor(
  owner: Owner,
  boot: Boot,
  snapshot: &str,
  rollback: &str,
  expected: Record,
) -> anyhow::Result<Prepared> {
  prepare_using(owner, boot, snapshot, rollback, Some(expected)).await
}

async fn prepare_using(
  owner: Owner,
  boot: Boot,
  snapshot: &str,
  rollback: &str,
  expected: Option<Record>,
) -> anyhow::Result<Prepared> {
  let Preparation { owner, result } =
    prepare_retained(owner, boot, snapshot, rollback, expected).await?;
  let (boot, restore) = result?;
  Ok(Prepared {
    owner,
    boot,
    restore,
  })
}

/// Trusted stopped-runtime preparation retaining the original owner on ordinary failure.
/// Retained ownership does not authorize startup or recovery without inspecting durable selection.
pub async fn prepare_retained(
  mut owner: Owner,
  boot: Boot,
  snapshot: &str,
  rollback: &str,
  expected: Option<Record>,
) -> anyhow::Result<Preparation> {
  let snapshot = snapshot.to_owned();
  let rollback = rollback.to_owned();
  let cancelled = Arc::new(AtomicBool::new(false));
  let _cancel = Cancel(cancelled.clone());
  let (completion, result) = tokio::sync::oneshot::channel();
  // the blocking owner retains both VM leases until validation/publication really finishes.
  tokio::task::spawn_blocking(move || {
    let prepared = (|| {
      owner.boot(&boot)?;
      let snapshot =
        model::native::machine_identity(&snapshot).context("Invalid restore snapshot")?;
      let rollback =
        model::native::machine_identity(&rollback).context("Invalid restore prior snapshot")?;
      ensure!(
        snapshot != rollback,
        "Restore requires a distinct recoverable prior state"
      );
      ensure!(
        owner.selected()? == expected,
        "Restore selection differs from preparation predecessor"
      );
      let scope = Scope {
        root: owner.directory()?,
        path: std::path::Path::new(&boot.store)
          .parent()
          .context("Missing restore namespace")?
          .to_owned(),
        policy: owner.policy(),
        cancelled,
        continuation: None,
      };
      stage::prepare(
        &mut owner,
        boot,
        snapshot,
        rollback,
        expected,
        scope,
        expected.map(|record| record.configuration),
      )
    })();
    let _ = completion.send(Preparation {
      owner,
      result: prepared,
    });
  });
  result
    .await
    .context("Restore preparation owner ended without completion")
}
