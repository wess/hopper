use super::*;
use scsi::snapshot::record;
use sha2::{Digest, Sha256};

fn media() -> (tempfile::TempDir, scsi::Optical) {
  let root = private();
  let path = root.path().join("source");
  fs::write(&path, [0x5a; 4096]).unwrap();
  let media = scsi::open(&path).unwrap();
  (root, media)
}

fn checksum(bytes: &mut [u8]) {
  let end = bytes.len() - 32;
  let checksum = Sha256::digest(&bytes[..end]);
  bytes[end..].copy_from_slice(&checksum);
}

#[test]
fn damaged_or_rechecksummed_invalid_controller_state_rejects_before_media_access() {
  let (_source, media) = media();
  let saved = scsi::snapshot::capture(&media).unwrap();
  let root = private();
  let directory = fs::File::open(root.path()).unwrap();
  let bytes = record::save(&saved, &directory, || Ok(())).unwrap();
  drop(saved);
  fs::remove_file(root.path().join("image")).unwrap();
  for length in [0, 4, 38, 2000, bytes.len() - 1] {
    assert!(record::load(&directory, &bytes[..length], 2, || Ok(())).is_err());
  }
  let mut changed = bytes;
  changed[39] ^= 1;
  assert!(record::load(&directory, &changed, 2, || Ok(())).is_err());
  let mut extended = bytes.to_vec();
  extended.push(0);
  assert!(record::load(&directory, &extended, 2, || Ok(())).is_err());
  for (offset, value) in [
    (4, 3),
    (13, 2),
    (16, 5),
    (20, 0x70),
    (38, 2),
    (2103, 1),
    (2119, 2),
    (2120, 1),
    (2128, 2),
    (2129, 1),
    (2145, 2),
    (2146, 1),
    (2162, 0),
  ] {
    let mut changed = bytes;
    changed[offset] = value;
    checksum(&mut changed);
    let error = record::load(&directory, &changed, 2, || Ok(()))
      .err()
      .unwrap();
    assert!(
      !error.to_string().contains("Open staged"),
      "metadata did not reject: {offset}: {error}"
    );
  }
  for (offset, payload) in [(2128, 2129), (2145, 2146)] {
    let mut changed = bytes;
    changed[offset] = 1;
    changed[payload] = 1;
    checksum(&mut changed);
    let error = record::load(&directory, &changed, 2, || Ok(()))
      .err()
      .unwrap();
    assert!(!error.to_string().contains("Open staged"));
  }
}

#[test]
fn optical_records_check_authorization_through_final_return() {
  let (_source, media) = media();
  let saved = scsi::snapshot::capture(&media).unwrap();
  let root = private();
  let directory = fs::File::open(root.path()).unwrap();
  let mut checks = 0;
  let bytes = record::save(&saved, &directory, || {
    checks += 1;
    Ok(())
  })
  .unwrap();
  let stage = private();
  let mut calls = 0;
  assert!(
    record::save(&saved, &fs::File::open(stage.path()).unwrap(), || {
      calls += 1;
      anyhow::ensure!(calls < checks, "revoked");
      Ok(())
    })
    .is_err()
  );
  let mut checks = 0;
  record::load(&directory, &bytes, 2, || {
    checks += 1;
    Ok(())
  })
  .unwrap();
  for stop in [1, checks] {
    let mut calls = 0;
    assert!(record::load(&directory, &bytes, 2, || {
      calls += 1;
      anyhow::ensure!(calls < stop, "revoked");
      Ok(())
    })
    .is_err());
  }
}

#[test]
fn optical_record_preserves_ejection_sense_and_sectors_across_processes() {
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
      .env("HOPPER_OPTICAL_RECORD_PHASE", phase)
      .env("HOPPER_OPTICAL_RECORD_ROOT", root.path())
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
fn minimum_and_maximum_negotiated_sizes_reopen_without_losing_pending_sense() {
  for (sense_size, cdb_size) in [(0, 6), (256, 256)] {
    let (_source, mut media) = media();
    scsi::configure(&mut media, 20, 4, sense_size);
    scsi::configure(&mut media, 24, 4, cdb_size);
    command(&mut media, &[0xff], 0);
    let saved = reopen(scsi::snapshot::capture(&media).unwrap());
    let mut restored = scsi::snapshot::restore(&saved, &media).unwrap();
    assert_eq!(
      (
        scsi::stats(&restored).sense_size,
        scsi::stats(&restored).cdb_size
      ),
      (sense_size, cdb_size)
    );
    let reply = command(&mut restored, &[3, 0, 0, 0, 18, 0], 18);
    assert_eq!((reply.0[0x3002], reply.0[0x300c]), (5, 0x20));
    let reply = command(&mut restored, &[3, 0, 0, 0, 18, 0], 18);
    assert_eq!((reply.0[0x3002], reply.0[0x300c]), (0, 0));
  }
}

#[test]
#[ignore = "invoked by separate-process optical record test"]
fn process_child() {
  use std::{
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
  };
  let phase = std::env::var("HOPPER_OPTICAL_RECORD_PHASE").unwrap();
  let root = std::path::PathBuf::from(std::env::var_os("HOPPER_OPTICAL_RECORD_ROOT").unwrap());
  let directory = fs::File::open(&root).unwrap();
  let (source, mut media) = media();
  if phase == "save" {
    scsi::configure(&mut media, 20, 4, 18);
    scsi::configure(&mut media, 24, 4, 16);
    command(&mut media, &[0x1b, 0, 0, 0, 2, 0], 0);
    command(&mut media, &[0], 0);
    let saved = scsi::snapshot::capture(&media).unwrap();
    let bytes = record::save(&saved, &directory, || Ok(())).unwrap();
    let mut file = fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(0o600)
      .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
      .open(root.join("controller"))
      .unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    directory.sync_all().unwrap();
    drop((saved, media, source));
  } else {
    assert_eq!(phase, "load");
    // target contents differ, so a fallback to them cannot pass the sector check.
    fs::write(source.path().join("source"), [0xe1; 4096]).unwrap();
    let mut bytes = Vec::new();
    fs::File::open(root.join("controller"))
      .unwrap()
      .take(record::SIZE as u64 + 1)
      .read_to_end(&mut bytes)
      .unwrap();
    let saved = record::load(&directory, &bytes, record::target(&media), || Ok(())).unwrap();
    let mut restored = scsi::snapshot::restore(&saved, &media).unwrap();
    assert_eq!(
      (
        scsi::stats(&restored).sense_size,
        scsi::stats(&restored).cdb_size
      ),
      (18, 16)
    );
    let sense = command(&mut restored, &[3, 0, 0, 0, 18, 0], 18);
    assert_eq!((sense.0[0x3002], sense.0[0x300c]), (2, 0x3a));
    assert_eq!(command(&mut restored, &[0], 0).0[0x200a], 2);
    command(&mut restored, &[0x1b, 0, 0, 0, 3, 0], 0);
    let read = command(&mut restored, &[0x28, 0, 0, 0, 0, 1, 0, 0, 1, 0], 2048);
    assert_eq!(&read.0[0x3000..0x3800], &[0x5a; 2048]);
  }
}
