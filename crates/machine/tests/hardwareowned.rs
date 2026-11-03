#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
use machine::{devices::virtio::block, runtime::hardware};
use model::native::Boot;
use std::{fs, os::unix::fs::symlink};

fn boot(root: &std::path::Path) -> Boot {
  Boot {
    machine_id: "8197e0f0-0603-43e9-a817-eaf7ab0327af".into(),
    firmware: root.join("firmware").to_string_lossy().into(),
    variables: root.join("template").to_string_lossy().into(),
    store: root.join("variables").to_string_lossy().into(),
    disk: None,
    disk_id: "hopper00000000000001".into(),
    memory: 0x10000000,
    cpus: 2,
    boot_media: None,
    installer: None,
    timeout_ms: None,
  }
}

#[test]
fn firmware_authorization_is_rechecked_during_bounded_reads() {
  let root = tempfile::tempdir().unwrap();
  fs::write(root.path().join("template"), vec![0xff; 1 << 20]).unwrap();
  let mut checks = 0;
  let error = hardware::original(boot(root.path()), || {
    checks += 1;
    anyhow::ensure!(checks < 4, "original policy revoked");
    Ok(())
  })
  .err()
  .unwrap();
  assert!(error.to_string().contains("original policy revoked"));
  assert!(!root.path().join("variables").exists());
}

#[test]
fn firmware_symlinks_and_special_files_reject_without_creating_state() {
  let root = tempfile::tempdir().unwrap();
  let template = root.path().join("template");
  fs::write(root.path().join("target"), [0xff]).unwrap();
  symlink(root.path().join("target"), &template).unwrap();
  assert!(hardware::original(boot(root.path()), || Ok(())).is_err());
  fs::remove_file(&template).unwrap();
  let name = std::ffi::CString::new(template.as_os_str().as_encoded_bytes()).unwrap();
  assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
  assert!(hardware::original(boot(root.path()), || Ok(())).is_err());
  assert!(!root.path().join("variables").exists());
  assert_eq!(fs::read(root.path().join("target")).unwrap(), [0xff]);
}

#[test]
fn current_hardware_uses_supplied_disk_without_opening_the_original_path() {
  let root = tempfile::tempdir().unwrap();
  fs::write(root.path().join("template"), [0x7f]).unwrap();
  fs::write(root.path().join("firmware"), 0x14000000u32.to_le_bytes()).unwrap();
  let disk = root.path().join("current");
  fs::write(&disk, [0; 4096]).unwrap();
  let current = block::open(&disk, false, *b"hopper00000000000001").unwrap();
  let mut config = boot(root.path());
  config.disk = Some(root.path().join("absent").to_string_lossy().into());
  let (_, devices, fingerprint) = hardware::current(config, Some(current), || Ok(())).unwrap();
  assert!(devices[4].is_some());
  assert_ne!(fingerprint, [0; 32]);
  assert!(!root.path().join("absent").exists());
  assert!(!root.path().join("variables").exists());
}
