#![cfg(unix)]

use engine::machines::windows::inspect;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

struct Fixture {
  root: tempfile::TempDir,
  tool: PathBuf,
  image: PathBuf,
}

impl Fixture {
  fn new(mode: &str) -> Self {
    let root = tempfile::tempdir().unwrap();
    let tool = root.path().join("tool");
    std::fs::write(&tool, include_str!("inspect/tool.py")).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(root.path().join("mode"), mode).unwrap();
    let image = root.path().join("installer.iso");
    std::fs::write(&image, b"unchanged source").unwrap();
    Self { root, tool, image }
  }

  async fn wait(&self, file: &str) {
    tokio::time::timeout(Duration::from_secs(3), async {
      while !self.root.path().join(file).exists() {
        tokio::time::sleep(Duration::from_millis(5)).await;
      }
    })
    .await
    .unwrap();
  }
}

#[test]
fn recovery_metadata_and_partition_capacity_are_bounded() {
  for text in [
    "",
    "Uncompressed size = 0 bytes",
    "Uncompressed size = 99 KB",
    "Uncompressed size = 4 bytes\nUncompressed size = 5 bytes",
  ] {
    assert!(inspect::recovery_size(text).is_err());
  }
  let bytes = inspect::recovery_size("Uncompressed size = 1200000000 bytes\n").unwrap();
  let contents = inspect::Contents {
    image_index: 7,
    recovery_image_bytes: bytes,
  };
  let layout = inspect::layout(&contents, 64).unwrap();
  assert_eq!(layout.image_index, 7);
  assert_eq!(layout.recovery_mib, 1459);
  assert!(inspect::layout(&contents, 1).is_err());
}

#[tokio::test]
async fn inspection_selects_actual_image_and_ejects_without_changing_source() {
  let f = Fixture::new("normal");
  let contents = inspect::read(&f.image, &f.tool, &f.tool).await.unwrap();
  assert_eq!(contents.image_index, 7);
  assert_eq!(contents.recovery_image_bytes, 1200000000);
  assert!(f.root.path().join("ejected").exists());
  let point = PathBuf::from(std::fs::read_to_string(f.root.path().join("point")).unwrap());
  assert!(!point.parent().unwrap().exists());
  assert_eq!(std::fs::read(&f.image).unwrap(), b"unchanged source");
}

#[tokio::test]
async fn malformed_metadata_still_ejects_the_owned_mount() {
  let f = Fixture::new("invalid");
  assert!(inspect::read(&f.image, &f.tool, &f.tool).await.is_err());
  assert!(f.root.path().join("ejected").exists());
}

#[tokio::test]
async fn caller_cancellation_ejects_after_helper_cleanup() {
  let f = std::sync::Arc::new(Fixture::new("hang"));
  let work = f.clone();
  let task = tokio::spawn(async move { inspect::read(&work.image, &work.tool, &work.tool).await });
  f.wait("waiting").await;
  task.abort();
  assert!(matches!(task.await, Err(error) if error.is_cancelled()));
  f.wait("ejected").await;
  let point = PathBuf::from(std::fs::read_to_string(f.root.path().join("point")).unwrap());
  assert!(!point.exists());
}

#[tokio::test]
async fn failed_ejection_preserves_the_mount_instead_of_traversing_it() {
  let f = Fixture::new("ejectfail");
  assert!(inspect::read(&f.image, &f.tool, &f.tool).await.is_err());
  let point = PathBuf::from(std::fs::read_to_string(f.root.path().join("point")).unwrap());
  assert_eq!(
    std::fs::read(point.join("sources/install.wim")).unwrap(),
    b"readonly image"
  );
  // this fixture is an ordinary directory, not a mounted filesystem.
  std::fs::remove_dir_all(point.parent().unwrap()).unwrap();
}
