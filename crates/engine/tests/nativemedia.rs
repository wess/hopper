#![cfg(unix)]

#[allow(dead_code)]
#[path = "media/fixture.rs"]
mod source;

use engine::machines::windows::media::{self, cache, convert};
use fs2::FileExt;
use std::os::unix::fs::{symlink, PermissionsExt};

#[tokio::test]
async fn cache_publication_and_reuse_validate_both_source_and_converted_image() {
  let f = source::Fixture::new();
  let root = f.root.path().join("machines");
  std::fs::create_dir(&root).unwrap();
  let iso = f.seed(&root).await;
  let mut phases = Vec::new();
  assert_eq!(
    media::prepare(&root, &f.tools, &root.join("progress"), |phase| phases
      .push(phase))
    .await
    .unwrap(),
    iso
  );
  assert!(matches!(phases.as_slice(), [media::Phase::VerifyingCache]));
  let bundle = iso.parent().unwrap();
  let second = f.stage("second");
  assert!(
    cache::publish(&second, bundle, &f.selection().await, &f.tools.archive)
      .await
      .is_err()
  );
  assert!(!second.join("manifest.json").exists());
  std::fs::write(&iso, [2; 32]).unwrap();
  assert!(
    media::prepare(&root, &f.tools, &root.join("progress"), |_| {})
      .await
      .is_err()
  );
  assert_eq!(std::fs::read(iso).unwrap(), [2; 32]);
}

#[tokio::test]
async fn cache_rejects_provenance_changes_public_files_symlinks_and_extra_entries() {
  for mode in ["source", "public", "symlink", "extra", "cab"] {
    let f = source::Fixture::new();
    let stage = f.stage("stage");
    let bundle = f.root.path().join("complete");
    let iso = cache::publish(&stage, &bundle, &f.selection().await, &f.tools.archive)
      .await
      .unwrap();
    match mode {
      "source" => {
        let path = bundle.join("manifest.json");
        let mut json: serde_json::Value =
          serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        json["sourceSha1"] = "b".repeat(40).into();
        std::fs::write(path, serde_json::to_vec(&json).unwrap()).unwrap();
      }
      "public" => std::fs::set_permissions(&iso, std::fs::Permissions::from_mode(0o644)).unwrap(),
      "symlink" => {
        std::fs::remove_file(&iso).unwrap();
        symlink("/usr/bin/true", &iso).unwrap();
      }
      "extra" => std::fs::write(bundle.join("extra"), b"unlisted").unwrap(),
      _ => std::fs::write(bundle.join("catalogue.cab"), b"modified CAB").unwrap(),
    }
    assert!(
      cache::verify(&bundle, &f.tools.archive).await.is_err(),
      "{mode}"
    );
  }
}

#[tokio::test]
async fn cache_cancellation_and_concurrent_ownership_never_publish_partial_media() {
  let f = source::Fixture::new();
  let selection = f.selection().await;
  let stage = f.stage("stage");
  let destination = f.root.path().join("complete");
  std::fs::write(f.root.path().join("mode"), "gate").unwrap();
  let mut task = Box::pin(cache::publish(
    &stage,
    &destination,
    &selection,
    &f.tools.archive,
  ));
  tokio::select! {
    result = &mut task => panic!("Unexpected publication: {result:?}"),
    _ = tokio::time::timeout(std::time::Duration::from_secs(3), async {
      while !f.root.path().join("waiting").exists() {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
      }
    }) => {
      assert!(f.root.path().join("waiting").exists());
    },
  }
  drop(task);
  assert!(!destination.exists());
  assert!(stage.join("installer.iso").exists());
  std::fs::write(f.root.path().join("mode"), "normal").unwrap();
  let root = f.root.path().join("machines");
  std::fs::create_dir(&root).unwrap();
  // seed uses a different staging directory after the abandoned publication.
  std::fs::rename(&stage, f.root.path().join("abandoned")).unwrap();
  f.seed(&root).await;
  let lock = std::fs::OpenOptions::new()
    .create(true)
    .truncate(false)
    .read(true)
    .write(true)
    .open(root.join("native/images/windows/lock"))
    .unwrap();
  lock.lock_exclusive().unwrap();
  assert!(
    media::prepare(&root, &f.tools, &root.join("progress"), |_| {})
      .await
      .unwrap_err()
      .to_string()
      .contains("Another VM")
  );
}

#[tokio::test]
async fn conversion_preserves_source_validates_edition_and_checks_each_export() {
  let f = source::Fixture::new();
  let esd = f.root.path().join("source.esd");
  std::fs::write(&esd, b"source remains intact").unwrap();
  let output = f.root.path().join("bundle");
  cache::directory(&output).unwrap();
  let iso = output.join("installer.iso");
  convert::build(&esd, &iso, &f.tools).await.unwrap();
  assert_eq!(std::fs::read(&iso).unwrap(), b"synthetic converted ISO");
  assert_eq!(std::fs::read(&esd).unwrap(), b"source remains intact");
  assert!(convert::build(&esd, &iso, &f.tools).await.is_err());
  let calls = std::fs::read_to_string(f.root.path().join("calls")).unwrap();
  let indices: Vec<String> = calls
    .lines()
    .map(|line| serde_json::from_str::<Vec<String>>(line).unwrap()[2].clone())
    .collect();
  assert_eq!(indices, ["1", "2", "3", "4", "5", "6"]);
  std::fs::remove_file(&iso).unwrap();
  std::fs::write(f.root.path().join("mode"), "invalid").unwrap();
  assert!(convert::build(&esd, &iso, &f.tools).await.is_err());
  assert!(!iso.exists());
  std::fs::write(f.root.path().join("mode"), "fail").unwrap();
  assert!(convert::build(&esd, &iso, &f.tools).await.is_err());
  assert!(!iso.exists());
  std::fs::write(f.root.path().join("mode"), "existing").unwrap();
  assert!(convert::build(&esd, &iso, &f.tools).await.is_err());
  assert!(!iso.exists());
}

#[test]
fn conversion_rejects_unbounded_or_ambiguous_source_image_counts() {
  assert_eq!(convert::count("Image Count: 6\n").unwrap(), 6);
  for value in [
    "",
    "Image Count: 3",
    "Image Count: 33",
    "Image Count: 6\nImage Count: 6",
  ] {
    assert!(convert::count(value).is_err());
  }
}
