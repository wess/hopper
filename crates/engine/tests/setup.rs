#![cfg(unix)]

#[path = "setup/fixture.rs"]
mod fixture;

use engine::machines::windows::setup::{archive, files};
use fixture::{Fixture, ID};
use std::{os::unix::fs::PermissionsExt, process, time::Duration};

#[test]
fn boot_member_selection_rejects_traversal_and_case_collisions() {
  let base = "EFI/MICROSOFT/BOOT/EFISYS.BIN\nSOURCES/BOOT.WIM\nBOOTMGR.EFI\nSOURCES/INSTALL.WIM";
  let members = archive::members(base).unwrap();
  assert_eq!(members.len(), 3);
  assert_eq!(members[1].1.to_str(), Some("sources/boot.wim"));
  for extra in ["EFI/../outside", "EFI/quoted\"file", "sources/boot.wim"] {
    assert!(archive::members(&format!("{base}\n{extra}")).is_err());
  }
  assert!(archive::members("EFI/boot/foo").is_err());
}

#[test]
fn driver_validation_checks_architecture_and_header_bounds() {
  let mut bytes = vec![0; 70];
  bytes[..2].copy_from_slice(b"MZ");
  bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
  bytes[64..68].copy_from_slice(b"PE\0\0");
  bytes[68..].copy_from_slice(&0xaa64u16.to_le_bytes());
  assert!(files::validate("driver.sys", &bytes).is_ok());
  bytes[68..].copy_from_slice(&0x8664u16.to_le_bytes());
  assert!(files::validate("driver.sys", &bytes).is_err());
  bytes[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
  assert!(files::validate("driver.sys", &bytes).is_err());
  assert!(files::validate("driver.inf", b"other architecture").is_err());
  assert!(files::validate("driver.cat", b"").is_err());
}

#[tokio::test]
async fn complete_bundle_is_private_and_never_replaced() {
  let f = Fixture::new("normal");
  f.build().await.unwrap();
  let manifest: serde_json::Value =
    serde_json::from_slice(&std::fs::read(f.output.with_extension("json")).unwrap()).unwrap();
  assert_eq!(manifest["vmId"], ID);
  assert_eq!(manifest["driverSha256"].as_object().unwrap().len(), 13);
  for path in [&f.output, &f.output.with_extension("json")] {
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
  }
  assert!(f.build().await.is_err());
  assert_eq!(
    std::fs::read(&f.output).unwrap(),
    b"completed private image"
  );
  assert_eq!(
    std::fs::read(f.root.path().join("installer.iso")).unwrap(),
    b"source image must remain unchanged"
  );
}

#[tokio::test]
async fn failed_update_leaves_no_published_bundle_and_can_retry() {
  let f = Fixture::new("fail");
  assert!(f.build().await.is_err());
  assert_eq!(f.output.parent().unwrap().read_dir().unwrap().count(), 0);
  std::fs::write(f.root.path().join("mode"), "normal").unwrap();
  f.build().await.unwrap();
}

#[tokio::test]
async fn oversized_helper_output_is_rejected_and_the_process_is_reaped() {
  let f = Fixture::new("flood");
  assert!(f.build().await.is_err());
  let pid = std::fs::read_to_string(f.root.path().join("pid")).unwrap();
  assert!(!process::Command::new("kill")
    .args(["-0", pid.trim()])
    .stderr(process::Stdio::null())
    .status()
    .unwrap()
    .success());
  assert_eq!(f.output.parent().unwrap().read_dir().unwrap().count(), 0);
}

#[tokio::test]
async fn cancellation_during_extraction_does_not_publish_partial_media() {
  let f = std::sync::Arc::new(Fixture::new("hang"));
  let work = f.clone();
  let pending = tokio::spawn(async move { work.build().await });
  tokio::time::timeout(Duration::from_secs(3), async {
    while !f.root.path().join("pid").exists() {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  pending.abort();
  assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
  let pid = std::fs::read_to_string(f.root.path().join("pid")).unwrap();
  tokio::time::timeout(Duration::from_secs(3), async {
    while process::Command::new("kill")
      .args(["-0", pid.trim()])
      .stderr(process::Stdio::null())
      .status()
      .unwrap()
      .success()
    {
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await
  .unwrap();
  assert_eq!(f.output.parent().unwrap().read_dir().unwrap().count(), 0);
}

#[tokio::test]
async fn data_added_during_build_is_not_overwritten_at_publication() {
  let f = Fixture::new("normal");
  let keep = f.output.parent().unwrap().join("keep");
  let result = engine::machines::windows::setup::build(&f.input(), &f.tools, |phase| {
    if matches!(phase, engine::machines::windows::setup::Phase::Building) {
      std::fs::write(&keep, b"preserve this data").unwrap();
    }
  })
  .await;
  assert!(result.is_err());
  assert_eq!(std::fs::read(keep).unwrap(), b"preserve this data");
  assert!(!f.output.exists() && !f.output.with_extension("json").exists());
}

#[test]
fn archive_catalogue_uses_types_even_when_directories_have_no_trailing_slash() {
  let names = "EFI\nEFI/MICROSOFT/BOOT/EFISYS.BIN\nSOURCES/BOOT.WIM\nEFI/EMPTY\nEFI/LINK";
  let metadata = "d directory\n- regular\n- regular\nd empty directory\nl symbolic link";
  let members = archive::catalogue(names, metadata).unwrap();
  assert_eq!(members.len(), 2);
  assert!(archive::catalogue(names, "- truncated").is_err());
}
