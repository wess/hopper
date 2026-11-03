#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[allow(dead_code)]
#[path = "restore/fixture.rs"]
mod fixture;
use engine::machines::native::{
  restore::{self, Journal, Phase, Record},
  snapshot::Policy,
};
use fixture::{Fixture, ID};
use sha2::{Digest, Sha256};
use std::{
  fs::{self, File, Permissions},
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
};

fn history(root: &Path, record: Record) -> PathBuf {
  root.join(format!(
    "restore{}.hrr",
    fixture::identity(record.generation)
  ))
}

fn sealed(path: &Path, bytes: &[u8]) {
  fs::write(path, bytes).unwrap();
  fs::set_permissions(path, Permissions::from_mode(0o400)).unwrap();
}

fn setup(phase: Phase) -> (Fixture, File, Record, Record) {
  let f = Fixture::new();
  let paths = f.initialize();
  fixture::artifacts(&paths.root);
  let expected = Record {
    phase,
    ..fixture::record()
  };
  sealed(&paths.root.join("restore.hrr"), &expected.encode().unwrap());
  let next = Record {
    generation: [6; 16],
    previous: Some(expected.generation),
    phase: Phase::Prepared,
    ..expected
  };
  let old = paths
    .root
    .join("generations")
    .join(fixture::identity(expected.generation));
  let new = paths
    .root
    .join("generations")
    .join(fixture::identity(next.generation));
  fixture::private(&new);
  let mut bytes = fs::read(old.join("generation.hgr")).unwrap();
  bytes[4..20].copy_from_slice(&next.generation);
  let end = bytes.len() - 32;
  let digest = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&digest);
  sealed(&new.join("generation.hgr"), &bytes);
  let root = File::open(&paths.root).unwrap();
  (f, root, expected, next)
}

#[test]
fn successor_preserves_each_predecessor_phase_and_survives_reopen() {
  for phase in [Phase::Prepared, Phase::Attempted, Phase::Active] {
    let (f, root, expected, next) = setup(phase);
    let path = restore_path(&f);
    let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
    let mut journal = Journal::open(&root, expected.machine).unwrap();
    journal
      .succeed(expected, next, fixture::layout(), &policy, || Ok(()))
      .unwrap();
    assert_eq!(
      fs::read(history(&path, expected)).unwrap(),
      expected.encode().unwrap()
    );
    assert_eq!(restore::read(&root, expected.machine).unwrap(), Some(next));
    let attempted = journal.attempted(next, &policy, || Ok(())).unwrap();
    assert_eq!(attempted.phase, Phase::Attempted);
    drop(journal);
    let reopened = Journal::open(&root, expected.machine).unwrap();
    assert_eq!(
      restore::read(&root, expected.machine).unwrap(),
      Some(attempted)
    );
    drop(reopened);
    assert!(path
      .join("generations")
      .join(fixture::identity(expected.generation))
      .exists());
  }
}

fn restore_path(f: &Fixture) -> PathBuf {
  engine::machines::native::config::paths(&f.manager.root, ID)
    .unwrap()
    .root
}

#[test]
fn cancellation_after_history_publication_leaves_old_selection_and_allows_retry() {
  let (f, root, expected, next) = setup(Phase::Attempted);
  let path = restore_path(&f);
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let mut journal = Journal::open(&root, expected.machine).unwrap();
  assert!(journal
    .succeed(expected, next, fixture::layout(), &policy, || {
      anyhow::ensure!(!history(&path, expected).exists(), "cancel after archival");
      Ok(())
    })
    .is_err());
  assert_eq!(
    restore::read(&root, expected.machine).unwrap(),
    Some(expected)
  );
  assert_eq!(
    fs::read(history(&path, expected)).unwrap(),
    expected.encode().unwrap()
  );
  journal
    .succeed(expected, next, fixture::layout(), &policy, || Ok(()))
    .unwrap();
  assert_eq!(restore::read(&root, expected.machine).unwrap(), Some(next));
}

#[test]
fn cancellation_after_successor_commit_keeps_new_recovery_intent() {
  let (f, root, expected, next) = setup(Phase::Active);
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let mut journal = Journal::open(&root, expected.machine).unwrap();
  assert!(journal
    .succeed(expected, next, fixture::layout(), &policy, || {
      anyhow::ensure!(
        restore::read(&root, expected.machine)? == Some(expected),
        "cancel after commit"
      );
      Ok(())
    })
    .is_err());
  assert_eq!(restore::read(&root, expected.machine).unwrap(), Some(next));
  assert_eq!(
    fs::read(history(&restore_path(&f), expected)).unwrap(),
    expected.encode().unwrap()
  );
}

#[test]
fn stale_binding_revocation_and_attempted_candidate_never_replace_predecessor() {
  for fault in 0..5 {
    let (f, root, expected, mut next) = setup(Phase::Active);
    let path = restore_path(&f);
    let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
    let mut journal = Journal::open(&root, expected.machine).unwrap();
    let mut compare = expected;
    match fault {
      0 => next.previous = None,
      1 => compare.phase = Phase::Attempted,
      2 => {
        f.manager.set_agent_access(ID, false).unwrap();
        f.manager.set_agent_access(ID, true).unwrap();
      }
      3 => fs::write(
        path
          .join("generations")
          .join(fixture::identity(next.generation))
          .join("attempt.hga"),
        b"already attempted",
      )
      .unwrap(),
      _ => sealed(&history(&path, next), &next.encode().unwrap()),
    }
    assert!(journal
      .succeed(compare, next, fixture::layout(), &policy, || Ok(()))
      .is_err());
    assert_eq!(
      restore::read(&root, expected.machine).unwrap(),
      Some(expected)
    );
    assert!(!history(&path, expected).exists());
  }
}

#[test]
fn conflicting_history_is_retained_without_replacing_selection() {
  let (f, root, expected, next) = setup(Phase::Attempted);
  let path = restore_path(&f);
  let archive = history(&path, expected);
  let conflict = Record {
    phase: Phase::Prepared,
    ..expected
  }
  .encode()
  .unwrap();
  sealed(&archive, &conflict);
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let mut journal = Journal::open(&root, expected.machine).unwrap();
  assert!(journal
    .succeed(expected, next, fixture::layout(), &policy, || Ok(()))
    .is_err());
  assert_eq!(
    restore::read(&root, expected.machine).unwrap(),
    Some(expected)
  );
  assert_eq!(fs::read(archive).unwrap(), conflict);
}

#[test]
#[ignore]
fn successor_crash_child() {
  let path = PathBuf::from(std::env::var_os("HOPPER_SUCCESSOR_ROOT").unwrap());
  let manager = engine::machines::Machines { root: path.clone() };
  let root = engine::machines::native::config::paths(&path, ID)
    .unwrap()
    .root;
  let expected = Record {
    phase: Phase::Attempted,
    ..fixture::record()
  };
  let next = Record {
    generation: [6; 16],
    previous: Some(expected.generation),
    phase: Phase::Prepared,
    ..expected
  };
  let mut owner = restore::Owner::agent(manager, ID, 0).unwrap();
  owner
    .succeed(expected, next, fixture::layout(), || {
      if history(&root, expected).exists() {
        std::process::exit(7);
      }
      Ok(())
    })
    .unwrap();
  panic!("crash boundary was not reached");
}

#[test]
fn process_exit_after_archival_preserves_recovery_and_releases_ownership() {
  let (f, root, expected, next) = setup(Phase::Attempted);
  let status = std::process::Command::new(std::env::current_exe().unwrap())
    .args(["--ignored", "--exact", "successor_crash_child"])
    .env("HOPPER_SUCCESSOR_ROOT", &f.manager.root)
    .status()
    .unwrap();
  assert_eq!(status.code(), Some(7));
  assert_eq!(
    restore::read(&root, expected.machine).unwrap(),
    Some(expected)
  );
  assert_eq!(
    fs::read(history(&restore_path(&f), expected)).unwrap(),
    expected.encode().unwrap()
  );
  let mut owner = restore::Owner::agent(f.manager.clone(), ID, 0).unwrap();
  owner
    .succeed(expected, next, fixture::layout(), || Ok(()))
    .unwrap();
  assert_eq!(owner.selected().unwrap(), Some(next));
}
