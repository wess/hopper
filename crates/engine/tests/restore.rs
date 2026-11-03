#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "restore/fixture.rs"]
mod fixture;
#[path = "restore/owner.rs"]
mod owner;
#[path = "restore/process.rs"]
mod process;
use engine::machines::native::{
  config::{self, Stage},
  restore::{self, Journal, Phase, Record},
  snapshot::Policy,
};
use fixture::{Fixture, ID};
use sha2::{Digest, Sha256};
use std::{
  fs::{self, File, Permissions},
  os::unix::fs::{symlink, FileExt, PermissionsExt},
};

#[test]
fn record_checks_identity_checksum_phase_and_exact_bounds() {
  let original = fixture::record();
  let encoded = original.encode().unwrap();
  assert_eq!(encoded.len(), 149);
  assert_eq!(
    Record::decode(&encoded, original.machine).unwrap(),
    original
  );
  for phase in [Phase::Prepared, Phase::Attempted, Phase::Active] {
    let record = Record { phase, ..original };
    assert_eq!(
      Record::decode(&record.encode().unwrap(), original.machine).unwrap(),
      record
    );
  }
  assert!(Record::decode(&encoded, [9; 16]).is_err());
  for length in [0, 4, 148, 150] {
    let mut bytes = encoded.clone();
    bytes.resize(length, 0);
    assert!(Record::decode(&bytes, original.machine).is_err());
  }
  for offset in [0, 4, 20, 36, 52, 84, 100, 116, 117, 148] {
    let mut bytes = encoded.clone();
    bytes[offset] ^= 1;
    assert!(Record::decode(&bytes, original.machine).is_err());
  }
  for (offset, value) in [(116, 9), (20, 0)] {
    let mut bytes = encoded.clone();
    if offset == 20 {
      bytes[20..36].fill(0);
    } else {
      bytes[offset] = value;
    }
    let digest = Sha256::digest(&bytes[..117]);
    bytes[117..].copy_from_slice(&digest);
    assert!(Record::decode(&bytes, original.machine).is_err());
  }
  for record in [
    Record {
      rollback: original.snapshot,
      ..original
    },
    Record {
      previous: Some(original.generation),
      ..original
    },
  ] {
    assert!(record.encode().is_err());
  }
}

#[test]
fn selection_and_attempt_survive_reopen_and_block_original_boot_paths() {
  let f = Fixture::new();
  let paths = f.initialize();
  std::fs::OpenOptions::new()
    .write(true)
    .open(&paths.disk)
    .unwrap()
    .write_all_at(b"boot", 0)
    .unwrap();
  config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
  fixture::artifacts(&paths.root);
  let root = File::open(&paths.root).unwrap();
  let record = fixture::record();
  let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
  let mut journal = Journal::open(&root, record.machine).unwrap();
  assert!(Journal::open(&root, record.machine).is_err());
  journal
    .select(record, fixture::layout(), &policy, || Ok(()))
    .unwrap();
  assert_eq!(restore::read(&root, record.machine).unwrap(), Some(record));
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System)
    .err()
    .unwrap()
    .to_string()
    .contains("restore"));
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::Deployment).is_err());
  assert!(config::initialize(&f.manager, ID).is_err());
  assert!(journal
    .select(record, fixture::layout(), &policy, || Ok(()))
    .is_err());
  let attempted = journal
    .attempted(record, &policy, || {
      if restore::read(&root, record.machine)?.is_some_and(|r| r.phase == Phase::Attempted) {
        anyhow::ensure!(
          f.manager.set_agent_access(ID, false).is_err(),
          "commit did not retain policy exclusion"
        );
      }
      Ok(())
    })
    .unwrap();
  assert_eq!(attempted.phase, Phase::Attempted);
  assert!(journal.attempted(record, &policy, || Ok(())).is_err());
  drop(journal);
  drop(root);
  let root = File::open(&paths.root).unwrap();
  assert_eq!(
    restore::read(&root, record.machine).unwrap(),
    Some(attempted)
  );
  assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System)
    .err()
    .unwrap()
    .to_string()
    .contains("restore"));
}

#[test]
fn missing_or_foreign_artifacts_and_revocation_do_not_publish_selection() {
  for fault in 0..5 {
    let f = Fixture::new();
    let paths = f.initialize();
    std::fs::OpenOptions::new()
      .write(true)
      .open(&paths.disk)
      .unwrap()
      .write_all_at(b"boot", 0)
      .unwrap();
    config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
    fixture::artifacts(&paths.root);
    let root = File::open(&paths.root).unwrap();
    let record = fixture::record();
    let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
    let mut journal = Journal::open(&root, record.machine).unwrap();
    if fault == 0 {
      fs::rename(
        paths
          .root
          .join("snapshots")
          .join(fixture::identity(record.rollback)),
        paths.root.join("retained"),
      )
      .unwrap();
    }
    if fault == 1 {
      let path = paths
        .root
        .join("generations")
        .join(fixture::identity(record.generation))
        .join("generation.hgr");
      fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
      fs::write(&path, b"foreign").unwrap();
      fs::set_permissions(path, Permissions::from_mode(0o400)).unwrap();
    }
    if fault == 3 {
      let path = paths
        .root
        .join("generations")
        .join(fixture::identity(record.generation))
        .join("generation.hgr");
      let mut bytes = fs::read(&path).unwrap();
      let end = bytes.len() - 32;
      bytes[end - 33] ^= 1;
      let manifest = Sha256::digest(&bytes[20..end - 32]);
      bytes[end - 32..end].copy_from_slice(&manifest);
      let checksum = Sha256::digest(&bytes[..end]);
      bytes[end..].copy_from_slice(&checksum);
      fs::set_permissions(&path, Permissions::from_mode(0o600)).unwrap();
      fs::write(&path, bytes).unwrap();
      fs::set_permissions(path, Permissions::from_mode(0o400)).unwrap();
    }
    if fault == 4 {
      fs::write(
        paths
          .root
          .join("generations")
          .join(fixture::identity(record.generation))
          .join("attempt.hga"),
        b"partial",
      )
      .unwrap();
    }
    if fault == 2 {
      f.manager.set_agent_access(ID, false).unwrap();
      f.manager.set_agent_access(ID, true).unwrap();
    }
    assert!(journal
      .select(record, fixture::layout(), &policy, || Ok(()))
      .is_err());
    assert!(restore::read(&root, record.machine).unwrap().is_none());
  }
}

#[test]
fn cancellation_after_selection_or_attempt_preserves_recovery_state() {
  for during_attempt in [false, true] {
    let f = Fixture::new();
    let paths = f.initialize();
    std::fs::OpenOptions::new()
      .write(true)
      .open(&paths.disk)
      .unwrap()
      .write_all_at(b"boot", 0)
      .unwrap();
    config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
    fixture::artifacts(&paths.root);
    let root = File::open(&paths.root).unwrap();
    let record = fixture::record();
    let policy = Policy::agent(f.manager.clone(), ID, 0).unwrap();
    let mut journal = Journal::open(&root, record.machine).unwrap();
    if during_attempt {
      journal
        .select(record, fixture::layout(), &policy, || Ok(()))
        .unwrap();
      assert!(journal
        .attempted(record, &policy, || {
          anyhow::ensure!(
            restore::read(&root, record.machine)?.is_some_and(|r| r.phase == Phase::Prepared),
            "cancelled after attempt commit"
          );
          Ok(())
        })
        .is_err());
    } else {
      assert!(journal
        .select(record, fixture::layout(), &policy, || {
          anyhow::ensure!(
            restore::read(&root, record.machine)?.is_none(),
            "cancelled after selection commit"
          );
          Ok(())
        })
        .is_err());
    }
    let saved = restore::read(&root, record.machine).unwrap().unwrap();
    assert_eq!(
      saved.phase,
      if during_attempt {
        Phase::Attempted
      } else {
        Phase::Prepared
      }
    );
    assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System)
      .err()
      .unwrap()
      .to_string()
      .contains("restore"));
    f.manager.set_agent_access(ID, false).unwrap();
  }
}

#[test]
fn malformed_foreign_symlink_and_public_records_never_fall_back_to_original_disk() {
  for fault in 0..4 {
    let f = Fixture::new();
    let paths = f.initialize();
    std::fs::OpenOptions::new()
      .write(true)
      .open(&paths.disk)
      .unwrap()
      .write_all_at(b"boot", 0)
      .unwrap();
    config::prepare(&f.manager, ID, &f.assets(), Stage::System).unwrap();
    let path = paths.root.join("restore.hrr");
    let record = fixture::record();
    let bytes = if fault == 0 {
      vec![0]
    } else {
      Record {
        machine: if fault == 1 { [9; 16] } else { record.machine },
        ..record
      }
      .encode()
      .unwrap()
    };
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(
      &path,
      Permissions::from_mode(if fault == 2 { 0o644 } else { 0o400 }),
    )
    .unwrap();
    if fault == 3 {
      fs::rename(&path, paths.root.join("retained")).unwrap();
      symlink(paths.root.join("retained"), &path).unwrap();
    }
    assert!(config::prepare(&f.manager, ID, &f.assets(), Stage::System)
      .err()
      .unwrap()
      .to_string()
      .contains("restore"));
    assert!(path.symlink_metadata().is_ok());
  }
}
