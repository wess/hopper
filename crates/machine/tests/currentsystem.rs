#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "current/fixture.rs"]
mod fixture;
use fixture::{sealed, Fixture};
use fs2::FileExt;
use machine::{
  runtime::variables,
  snapshot::{generation::System, selection::Phase},
};
use std::{
  fs::{self, OpenOptions},
  os::unix::fs::FileExt as _,
};

#[test]
fn fresh_system_admission_uses_mutable_resources_without_loading_old_memory() {
  let f = Fixture::new();
  let mut store = variables::open(&f.generation.join("variables")).unwrap();
  variables::commit(&mut store, 0, &[7; 512]).unwrap();
  drop(store);
  let probe = OpenOptions::new()
    .read(true)
    .write(true)
    .open(f.generation.join("storage0/disk"))
    .unwrap();
  probe.write_all_at(b"current", 0).unwrap();
  let before = fs::read(f.generation.join("generation.hgr")).unwrap();
  let system = System::open(&f.open(), f.record, || Ok(())).unwrap();
  assert_eq!(system.layout().devices, [1, 0, 0, 0, 0, 0, 0]);
  assert!(probe.try_lock_exclusive().is_err());
  assert!(System::open(&f.open(), f.record, || Ok(())).is_err());
  let (store, mut disks) = system.admit(|| Ok(())).unwrap();
  assert_eq!(&variables::bytes(&store)[..512], &[7; 512]);
  let current = disks[0].take().unwrap();
  let mut bytes = [0; 7];
  current.file().read_exact_at(&mut bytes, 0).unwrap();
  assert_eq!(&bytes, b"current");
  let disk = current.disk([b'd'; 20], || Ok(())).unwrap();
  assert!(probe.try_lock_exclusive().is_err());
  drop(disk);
  drop(store);
  probe.try_lock_exclusive().unwrap();
  FileExt::unlock(&probe).unwrap();
  assert_eq!(
    fs::read(f.generation.join("generation.hgr")).unwrap(),
    before
  );
  assert!(!f.root.path().join("snapshots").exists());
  drop(System::open(&f.open(), f.record, || Ok(())).unwrap());
}

#[test]
fn inactive_missing_corrupt_and_foreign_bindings_reject_without_repair() {
  use sha2::{Digest, Sha256};
  let f = Fixture::new();
  for phase in [Phase::Prepared, Phase::Attempted] {
    let record = machine::snapshot::selection::Record { phase, ..f.record };
    sealed(
      &f.root.path().join("restore.hrr"),
      &record.encode().unwrap(),
    );
    assert!(System::open(&f.open(), record, || Ok(())).is_err());
  }
  sealed(
    &f.root.path().join("restore.hrr"),
    &f.record.encode().unwrap(),
  );
  let marker = fs::read(f.generation.join("attempt.hga")).unwrap();
  for bytes in [&b"HGA"[..], &[0; 84][..], &marker[..83]] {
    sealed(&f.generation.join("attempt.hga"), bytes);
    assert!(System::open(&f.open(), f.record, || Ok(())).is_err());
    assert_eq!(fs::read(f.generation.join("attempt.hga")).unwrap(), bytes);
  }
  sealed(&f.generation.join("attempt.hga"), &marker);
  let receipt = fs::read(f.generation.join("generation.hgr")).unwrap();
  let mut foreign = receipt.clone();
  foreign[4..20].fill(9);
  let end = foreign.len() - 32;
  let hash = Sha256::digest(&foreign[..end]);
  foreign[end..].copy_from_slice(&hash);
  sealed(&f.generation.join("generation.hgr"), &foreign);
  assert!(System::open(&f.open(), f.record, || Ok(())).is_err());
  sealed(&f.generation.join("generation.hgr"), &receipt);
  fs::remove_file(f.generation.join("attempt.hga")).unwrap();
  assert!(System::open(&f.open(), f.record, || Ok(())).is_err());
  assert!(!f.generation.join("attempt.hga").exists());
}

#[test]
fn namespace_replacement_and_revocation_release_owners_without_redirecting() {
  let f = Fixture::new();
  let system = System::open(&f.open(), f.record, || Ok(())).unwrap();
  let duplicate = OpenOptions::new()
    .read(true)
    .write(true)
    .open(f.generation.join("storage0/disk"))
    .unwrap();
  assert!(duplicate.try_lock_exclusive().is_err());
  fs::rename(f.generation.join("variables"), f.generation.join("saved")).unwrap();
  fixture::private(&f.generation.join("variables"));
  assert!(system.admit(|| Ok(())).is_err());
  duplicate.try_lock_exclusive().unwrap();
  FileExt::unlock(&duplicate).unwrap();
  fs::remove_dir(f.generation.join("variables")).unwrap();
  fs::rename(f.generation.join("saved"), f.generation.join("variables")).unwrap();
  let system = System::open(&f.open(), f.record, || Ok(())).unwrap();
  assert!(system.admit(|| anyhow::bail!("revoked")).is_err());
  duplicate.try_lock_exclusive().unwrap();
  FileExt::unlock(&duplicate).unwrap();
  drop(System::open(&f.open(), f.record, || Ok(())).unwrap());
}

#[test]
fn changed_selection_and_disk_namespace_reject_owned_handoff() {
  let f = Fixture::new();
  let system = System::open(&f.open(), f.record, || Ok(())).unwrap();
  let mut other = f.record;
  other.rollback = [9; 16];
  sealed(&f.root.path().join("restore.hrr"), &other.encode().unwrap());
  assert!(system.check(|| Ok(())).is_err());
  sealed(
    &f.root.path().join("restore.hrr"),
    &f.record.encode().unwrap(),
  );
  fs::rename(f.generation.join("storage0"), f.generation.join("saved")).unwrap();
  fixture::private(&f.generation.join("storage0"));
  assert!(system.admit(|| Ok(())).is_err());
  assert!(fs::read_dir(f.generation.join("storage0"))
    .unwrap()
    .next()
    .is_none());
}

#[test]
fn durable_ownership_rejects_same_size_disk_replacement_and_missing_receipts() {
  use std::os::unix::fs::OpenOptionsExt;
  let f = Fixture::new();
  let path = f.generation.join("storage0/disk");
  fs::rename(&path, f.generation.join("storage0/saved")).unwrap();
  let replacement = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(&path)
    .unwrap();
  replacement.set_len(4096).unwrap();
  assert!(System::open(&f.open(), f.record, || Ok(())).is_err());
  fs::remove_file(&path).unwrap();
  fs::rename(f.generation.join("storage0/saved"), &path).unwrap();
  drop(System::open(&f.open(), f.record, || Ok(())).unwrap());
  fs::remove_file(f.generation.join("ownership.hgo")).unwrap();
  assert!(System::open(&f.open(), f.record, || Ok(())).is_err());
  assert!(!f.generation.join("ownership.hgo").exists());
}
