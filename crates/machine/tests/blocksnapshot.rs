#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod support;
use machine::devices::virtio::{
  block::{self, snapshot},
  queue,
};
use std::os::unix::fs::FileExt;
use support::{descriptor, Ram, BASE};

#[path = "storage/blockrecord.rs"]
mod record;

fn private() -> tempfile::TempDir {
  use std::os::unix::fs::PermissionsExt;
  tempfile::Builder::new()
    .permissions(std::fs::Permissions::from_mode(0o700))
    .tempdir()
    .unwrap()
}

fn reopen(saved: snapshot::State) -> snapshot::State {
  let root = private();
  let directory = std::fs::File::open(root.path()).unwrap();
  let expected = snapshot::record::binding(&saved);
  let bytes = snapshot::record::save(&saved, &directory, || Ok(())).unwrap();
  drop(saved);
  snapshot::record::load(&directory, &bytes, expected, || Ok(())).unwrap()
}

fn request(disk: &mut block::Disk, ram: &mut Ram, kind: u32) {
  assert_eq!(exchange(disk, ram, kind), 0);
}

fn exchange(disk: &mut block::Disk, ram: &mut Ram, kind: u32) -> u8 {
  ram.0[0x1000..0x1010].fill(0);
  ram.0[0x1000..0x1004].copy_from_slice(&kind.to_le_bytes());
  descriptor(ram, 0, BASE + 0x1000, 16, 1, 1);
  descriptor(ram, 1, BASE + 0x2000, 512, if kind == 0 { 3 } else { 1 }, 2);
  descriptor(ram, 2, BASE + 0x3000, 1, 2, 0);
  ram.0[0x102..0x104].copy_from_slice(&1u16.to_le_bytes());
  ram.0[0x104..0x106].copy_from_slice(&0u16.to_le_bytes());
  ram.0[0x202..0x204].fill(0);
  let mut queue = queue::create(ram, 8, BASE, BASE + 0x100, BASE + 0x200).unwrap();
  let chain = queue::pop(&mut queue, ram).unwrap().unwrap();
  let written = block::execute(disk, ram, &chain).unwrap();
  queue::complete(&mut queue, ram, &chain, written).unwrap();
  ram.0[0x3000]
}

fn disk(id: u8, readonly: bool) -> block::Disk {
  let file = tempfile::tempfile().unwrap();
  file.set_len(65536).unwrap();
  block::attach(file, readonly, [id; 20]).unwrap()
}

#[test]
fn recovered_disk_keeps_identity_and_uses_independent_private_images() {
  let mut original = disk(b'd', false);
  let mut ram = Ram::default();
  block::writeback(&mut original, true).unwrap();
  ram.0[0x2000..0x2200].fill(b'a');
  request(&mut original, &mut ram, 1);
  let saved = reopen(snapshot::capture(&original).unwrap());
  ram.0[0x2000..0x2200].fill(b'b');
  request(&mut original, &mut ram, 1);
  drop(original);
  let file = tempfile::tempfile().unwrap();
  file.set_len(65536).unwrap();
  let retained = file.try_clone().unwrap();
  let target = block::attach(file, false, [b'd'; 20]).unwrap();
  for _ in 0..2 {
    let mut restored = snapshot::restore(&saved, &target).unwrap();
    assert_eq!(block::capacity(&restored), 128);
    request(&mut restored, &mut ram, 0);
    assert_eq!(&ram.0[0x2000..0x2200], &[b'a'; 512]);
    ram.0[0x2000..0x2200].fill(b'c');
    request(&mut restored, &mut ram, 1);
    request(&mut restored, &mut ram, 0);
    assert_eq!(&ram.0[0x2000..0x2200], &[b'c'; 512]);
  }
  let mut bytes = [1; 512];
  retained.read_exact_at(&mut bytes, 0).unwrap();
  assert_eq!(bytes, [0; 512]);
  assert!(snapshot::restore(&saved, &disk(b'e', false)).is_err());
  assert!(snapshot::restore(&saved, &disk(b'd', true)).is_err());
  let file = tempfile::tempfile().unwrap();
  file.set_len(512).unwrap();
  let smaller = block::attach(file, false, [b'd'; 20]).unwrap();
  assert!(snapshot::restore(&saved, &smaller).is_err());
}

#[test]
fn readonly_recovery_rejects_guest_writes_and_keeps_saved_contents() {
  let file = tempfile::tempfile().unwrap();
  file.set_len(65536).unwrap();
  file.write_all_at(&[b'r'; 512], 0).unwrap();
  let original = block::attach(file, true, [b'r'; 20]).unwrap();
  let saved = reopen(snapshot::capture(&original).unwrap());
  drop(original);
  let target = disk(b'r', true);
  let mut restored = snapshot::restore(&saved, &target).unwrap();
  let mut ram = Ram::default();
  request(&mut restored, &mut ram, 0);
  assert_eq!(&ram.0[0x2000..0x2200], &[b'r'; 512]);
  ram.0[0x2000..0x2200].fill(b'x');
  assert_eq!(exchange(&mut restored, &mut ram, 1), 1);
  request(&mut restored, &mut ram, 0);
  assert_eq!(&ram.0[0x2000..0x2200], &[b'r'; 512]);
}

#[test]
fn dropping_controller_releases_owned_lock_despite_a_duplicate_descriptor() {
  let file = tempfile::NamedTempFile::new().unwrap();
  file.as_file().set_len(512).unwrap();
  fs2::FileExt::try_lock_exclusive(file.as_file()).unwrap();
  let duplicate = file.as_file().try_clone().unwrap();
  let owned = block::attach(duplicate, false, [b'l'; 20]).unwrap();
  let contender = std::fs::File::open(file.path()).unwrap();
  assert!(fs2::FileExt::try_lock_exclusive(&contender).is_err());
  drop(owned);
  fs2::FileExt::try_lock_exclusive(&contender).unwrap();
  fs2::FileExt::unlock(&contender).unwrap();
}
