#![cfg(target_os = "linux")]

use guest::files::upload;
use model::guest::files::COMMIT;
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor};

#[test]
fn multiple_binary_chunks_publish_only_after_complete_checked_commit() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("new");
  let bytes = (0..3 * guest::files::CHUNK + 17)
    .map(|index| index as u8)
    .collect::<Vec<_>>();
  let digest = format!("{:x}", Sha256::digest(&bytes));
  let mut source = bytes.clone();
  source.extend_from_slice(COMMIT);
  let result =
    upload::receive(&path, bytes.len() as u64, &digest, &mut Cursor::new(source)).unwrap();
  assert_eq!(result.sha256, digest);
  assert_eq!(result.file.length, bytes.len());
  assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn incomplete_uncommitted_trailing_and_corrupt_data_never_publish() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("new");
  let bytes = b"whole payload";
  let digest = format!("{:x}", Sha256::digest(bytes));
  let mut committed = bytes.to_vec();
  committed.extend_from_slice(COMMIT);
  let mut trailing = committed.clone();
  trailing.push(0);
  let mut corrupt = committed.clone();
  corrupt[0] ^= 1;
  let mut wrong_marker = committed;
  wrong_marker[bytes.len()] ^= 1;
  for source in [
    bytes[..4].to_vec(),
    bytes.to_vec(),
    trailing,
    corrupt,
    wrong_marker,
  ] {
    assert!(upload::receive(&path, bytes.len() as u64, &digest, &mut Cursor::new(source)).is_err());
    assert!(!path.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
  }
}

#[test]
fn production_stream_cli_keeps_checksum_and_payload_in_its_private_channel() {
  use std::{
    io::Write,
    net::Shutdown,
    os::fd::OwnedFd,
    os::unix::net::UnixStream,
    process::{Command, Stdio},
  };
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("new");
  let bytes = vec![255; 3 * guest::files::CHUNK + 17];
  let hash = Sha256::digest(&bytes);
  let (receiver, mut sender) = UnixStream::pair().unwrap();
  let input: OwnedFd = receiver.into();
  let expected = bytes.clone();
  let source = std::thread::spawn(move || {
    sender.write_all(&hash).unwrap();
    sender.write_all(&bytes).unwrap();
    sender.write_all(COMMIT).unwrap();
    sender.shutdown(Shutdown::Write).unwrap();
  });
  let result = Command::new(
    std::env::var_os("HOPPER_GUEST_TEST_EXECUTABLE")
      .unwrap_or_else(|| env!("CARGO_BIN_EXE_hopperguest").into()),
  )
  .arg("--upload-file")
  .arg(&path)
  .arg(expected.len().to_string())
  .stdin(Stdio::from(input))
  .output()
  .unwrap();
  source.join().unwrap();
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let receipt: model::guest::files::Uploaded = serde_json::from_slice(&result.stdout).unwrap();
  assert_eq!(receipt.file.length, expected.len());
  assert_eq!(fs::read(path).unwrap(), expected);
}

#[test]
fn replaced_parent_cannot_redirect_publication() {
  use std::{io::Read, path::PathBuf};
  struct Source {
    bytes: Cursor<Vec<u8>>,
    parent: PathBuf,
    retained: PathBuf,
    size: u64,
    moved: bool,
  }
  impl Read for Source {
    fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
      if !self.moved && self.bytes.position() >= self.size {
        fs::rename(&self.parent, &self.retained)?;
        fs::create_dir(&self.parent)?;
        self.moved = true;
      }
      self.bytes.read(target)
    }
  }
  let root = tempfile::tempdir().unwrap();
  let parent = root.path().join("parent");
  let retained = root.path().join("retained");
  fs::create_dir(&parent).unwrap();
  let bytes = b"payload";
  let mut input = bytes.to_vec();
  input.extend_from_slice(COMMIT);
  let mut source = Source {
    bytes: Cursor::new(input),
    parent: parent.clone(),
    retained: retained.clone(),
    size: bytes.len() as u64,
    moved: false,
  };
  let digest = format!("{:x}", Sha256::digest(bytes));
  assert!(upload::receive(
    &parent.join("new"),
    bytes.len() as u64,
    &digest,
    &mut source
  )
  .is_err());
  assert!(source.moved);
  assert_eq!(fs::read_dir(parent).unwrap().count(), 0);
  assert_eq!(fs::read_dir(retained).unwrap().count(), 0);
}
