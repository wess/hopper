#![cfg(target_os = "linux")]

use guest::files::create;
use std::{
  fs,
  os::unix::fs::{symlink, MetadataExt},
};

#[test]
fn atomic_creation_preserves_existing_content_and_publishes_private_binary_bytes() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("new");
  let bytes = (0..model::guest::files::CREATE_LIMIT)
    .map(|index| index as u8)
    .collect::<Vec<_>>();
  let receipt = create::write(&path, &bytes).unwrap();
  let metadata = fs::metadata(&path).unwrap();
  assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
  assert_eq!(metadata.mode() & 0o7777, 0o600);
  assert_eq!(metadata.nlink(), 1);
  assert_eq!(receipt.stamp.inode, metadata.ino());
  assert_eq!(receipt.length, bytes.len());
  assert!(receipt.eof && receipt.stamp.valid());
  assert_eq!(fs::read(&path).unwrap(), bytes);
  assert!(create::write(&path, b"replacement").is_err());
  assert_eq!(fs::metadata(&path).unwrap().ino(), metadata.ino());
  assert_eq!(fs::read(&path).unwrap(), bytes);
  assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn symlink_destinations_and_parents_preserve_their_targets() {
  let root = tempfile::tempdir().unwrap();
  let target = root.path().join("target");
  fs::write(&target, b"preserve").unwrap();
  let link = root.path().join("link");
  symlink(&target, &link).unwrap();
  assert!(create::write(&link, b"replacement").is_err());
  assert_eq!(fs::read(&target).unwrap(), b"preserve");
  let directory = root.path().join("directory");
  fs::create_dir(&directory).unwrap();
  let alias = root.path().join("alias");
  symlink(&directory, &alias).unwrap();
  assert!(create::write(&alias.join("new"), b"bytes").is_err());
  assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
}

#[test]
fn invalid_requests_create_no_destination_or_staging_names() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("new");
  assert!(create::write(&path, &vec![0; model::guest::files::CREATE_LIMIT + 1]).is_err());
  assert!(create::write(std::path::Path::new("relative"), b"bytes").is_err());
  assert!(create::write(&root.path().join("."), b"bytes").is_err());
  assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn production_cli_accepts_only_the_complete_sealed_payload() {
  use std::{
    io::{Seek, SeekFrom, Write},
    os::fd::{AsRawFd, FromRawFd},
    process::Command,
  };
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("new");
  let bytes = b"binary\0payload\xff";
  let fd = unsafe {
    libc::memfd_create(
      c"test.payload".as_ptr(),
      libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
    )
  };
  assert!(fd >= 0);
  let mut input = unsafe { fs::File::from_raw_fd(fd) };
  input.write_all(bytes).unwrap();
  input.seek(SeekFrom::Start(0)).unwrap();
  let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
  assert_eq!(
    unsafe { libc::fcntl(input.as_raw_fd(), libc::F_ADD_SEALS, seals) },
    0
  );
  let result = Command::new(
    std::env::var_os("HOPPER_GUEST_TEST_EXECUTABLE")
      .unwrap_or_else(|| env!("CARGO_BIN_EXE_hopperguest").into()),
  )
  .arg("--create-file")
  .arg(&path)
  .arg(bytes.len().to_string())
  .stdin(input)
  .output()
  .unwrap();
  assert!(
    result.status.success(),
    "{}",
    String::from_utf8_lossy(&result.stderr)
  );
  let receipt: model::guest::files::Receipt = serde_json::from_slice(&result.stdout).unwrap();
  assert_eq!(receipt.length, bytes.len());
  assert_eq!(fs::read(&path).unwrap(), bytes);
  let rejected = root.path().join("rejected");
  let mut input = tempfile::tempfile().unwrap();
  input.write_all(bytes).unwrap();
  input.seek(SeekFrom::Start(0)).unwrap();
  let result = Command::new(
    std::env::var_os("HOPPER_GUEST_TEST_EXECUTABLE")
      .unwrap_or_else(|| env!("CARGO_BIN_EXE_hopperguest").into()),
  )
  .arg("--create-file")
  .arg(&rejected)
  .arg(bytes.len().to_string())
  .stdin(input)
  .output()
  .unwrap();
  assert!(!result.status.success());
  assert!(!rejected.exists());
}
