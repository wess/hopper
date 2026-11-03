use super::*;
use sha2::{Digest, Sha256};

#[test]
fn invalid_or_foreign_controller_records_reject_before_image_access() {
  let saved = snapshot::capture(&disk(b'd', false)).unwrap();
  let root = private();
  let directory = std::fs::File::open(root.path()).unwrap();
  let bytes = snapshot::record::save(&saved, &directory, || Ok(())).unwrap();
  let expected = snapshot::record::binding(&saved);
  drop(saved);
  std::fs::remove_file(root.path().join("image")).unwrap();
  for (offset, value) in [(4, 0), (12, 2), (13, 2), (14, b'e')] {
    let mut changed = bytes;
    changed[offset] = value;
    let digest = Sha256::digest(&changed[..110]);
    changed[110..].copy_from_slice(&digest);
    let error = snapshot::record::load(&directory, &changed, expected, || Ok(()))
      .err()
      .unwrap();
    assert!(!error.to_string().contains("Open staged"));
  }
  for length in [0, 4, 100, bytes.len() - 1] {
    assert!(snapshot::record::load(&directory, &bytes[..length], expected, || Ok(())).is_err());
  }
  let mut changed = bytes;
  changed[13] ^= 1;
  assert!(snapshot::record::load(&directory, &changed, expected, || Ok(())).is_err());
}

#[test]
fn pci_disk_reopen_drops_original_image_and_preserves_controller_capacity() {
  use machine::devices::virtio::pci;
  let mut original = disk(b'd', false);
  block::writeback(&mut original, true).unwrap();
  let device = pci::create(original).unwrap();
  let saved = pci::snapshot::capture(&device).unwrap();
  let transport = pci::snapshot::wire::encode(&saved).unwrap();
  drop(device);
  let root = private();
  let directory = std::fs::File::open(root.path()).unwrap();
  let bytes = pci::snapshot::storagewire::save(&saved, &directory, || Ok(())).unwrap();
  assert_eq!(bytes[13], 1);
  drop(saved);
  let mut restored = pci::create(disk(b'd', false)).unwrap();
  let saved = pci::snapshot::records::decode(
    &transport,
    pci::snapshot::records::Contents::Image {
      directory: &directory,
      bytes: &bytes,
    },
    &restored,
    || Ok(()),
  )
  .unwrap();
  pci::snapshot::restore(&saved, &mut restored, &Ram::default()).unwrap();
  assert_eq!(pci::read(&mut restored, pci::SPECIFIC, 4).unwrap(), 128);
}

#[test]
fn disk_record_recovers_in_separate_processes_after_original_owner_exits() {
  let root = private();
  let executable = std::env::current_exe().unwrap();
  for phase in ["save", "load", "load"] {
    let result = std::process::Command::new(&executable)
      .args([
        "--exact",
        "record::process_child",
        "--ignored",
        "--nocapture",
      ])
      .env("HOPPER_DISK_RECORD_PHASE", phase)
      .env("HOPPER_DISK_RECORD_ROOT", root.path())
      .output()
      .unwrap();
    assert!(
      result.status.success(),
      "child {phase}: {}{}",
      String::from_utf8_lossy(&result.stdout),
      String::from_utf8_lossy(&result.stderr)
    );
  }
}

#[test]
#[ignore = "invoked by separate-process disk record test"]
fn process_child() {
  use std::{
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
  };
  let phase = std::env::var("HOPPER_DISK_RECORD_PHASE").unwrap();
  let root = std::path::PathBuf::from(std::env::var_os("HOPPER_DISK_RECORD_ROOT").unwrap());
  let directory = std::fs::File::open(&root).unwrap();
  let mut ram = Ram::default();
  if phase == "save" {
    let mut disk = disk(b'd', false);
    block::writeback(&mut disk, true).unwrap();
    ram.0[0x2000..0x2200].fill(b'a');
    request(&mut disk, &mut ram, 1);
    let saved = snapshot::capture(&disk).unwrap();
    let bytes = snapshot::record::save(&saved, &directory, || Ok(())).unwrap();
    let mut file = std::fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
      .open(root.join("controller"))
      .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    directory.sync_all().unwrap();
    // the source image and ephemeral capture disappear with this process.
    drop((saved, disk));
  } else {
    assert_eq!(phase, "load");
    let target = disk(b'd', false);
    let mut bytes = Vec::new();
    std::fs::File::open(root.join("controller"))
      .unwrap()
      .take(snapshot::record::SIZE as u64 + 1)
      .read_to_end(&mut bytes)
      .unwrap();
    let saved = snapshot::record::load(
      &directory,
      &bytes,
      snapshot::record::target(&target),
      || Ok(()),
    )
    .unwrap();
    let mut recovered = snapshot::restore(&saved, &target).unwrap();
    request(&mut recovered, &mut ram, 0);
    assert_eq!(&ram.0[0x2000..0x2200], &[b'a'; 512]);
    ram.0[0x2000..0x2200].fill(b'b');
    request(&mut recovered, &mut ram, 1);
    request(&mut recovered, &mut ram, 0);
    assert_eq!(&ram.0[0x2000..0x2200], &[b'b'; 512]);
  }
}
