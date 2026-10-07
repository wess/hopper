use engine::machines::windows::deploy::{self, Layout};
use std::io::{Read, Seek, SeekFrom, Write};

fn layout() -> Layout {
  Layout {
    disk_gib: 64,
    image_index: 6,
    recovery_mib: 2048,
    recovery_image_bytes: 900 * 1024 * 1024,
  }
}

#[test]
fn deployment_rejects_bad_geometry_and_insufficient_recovery_space() {
  for disk_gib in [0, 63, 2049, u32::MAX] {
    assert!(deploy::prepare(&Layout {
      disk_gib,
      ..layout()
    })
    .is_err());
  }
  for image_index in [0, 33, u32::MAX] {
    assert!(deploy::prepare(&Layout {
      image_index,
      ..layout()
    })
    .is_err());
  }
  for recovery_mib in [0, 1023, 4097, u32::MAX] {
    assert!(deploy::prepare(&Layout {
      recovery_mib,
      ..layout()
    })
    .is_err());
  }
  for recovery_image_bytes in [0, 2048 * 1024 * 1024, u64::MAX] {
    assert!(deploy::prepare(&Layout {
      recovery_image_bytes,
      ..layout()
    })
    .is_err());
  }
}

#[test]
fn new_sparse_disk_is_locked_and_existing_data_is_preserved() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("disk");
  let mut disk = deploy::create_disk(&path, &layout()).unwrap();
  assert_eq!(disk.metadata().unwrap().len(), 64 * 1024 * 1024 * 1024);
  #[cfg(unix)]
  {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    assert_eq!(disk.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    assert!(disk.metadata().unwrap().blocks() * 512 < 1024 * 1024);
  }
  let peer = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(&path)
    .unwrap();
  assert!(fs2::FileExt::try_lock_exclusive(&peer).is_err());
  disk.seek(SeekFrom::Start(0)).unwrap();
  disk.write_all(b"existing guest data").unwrap();
  assert!(deploy::create_disk(&path, &layout()).is_err());
  disk.seek(SeekFrom::Start(0)).unwrap();
  let mut bytes = [0; 19];
  disk.read_exact(&mut bytes).unwrap();
  assert_eq!(&bytes, b"existing guest data");
  drop(disk);
  fs2::FileExt::try_lock_exclusive(&peer).unwrap();
}

#[test]
fn invalid_plan_never_creates_a_disk() {
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("disk");
  assert!(deploy::create_disk(
    &path,
    &Layout {
      image_index: 0,
      ..layout()
    }
  )
  .is_err());
  assert!(!path.exists());
}

#[test]
fn scripts_target_the_new_disk_and_stop_before_readiness_on_failure() {
  let plan = deploy::prepare(&layout()).unwrap();
  assert!(plan
    .partitions
    .starts_with("select disk 0\r\nclean\r\nconvert gpt\r\n"));
  assert!(plan
    .partitions
    .contains("create partition efi size=512\r\n"));
  assert!(plan.partitions.contains("create partition msr size=16\r\n"));
  assert!(plan
    .partitions
    .contains("create partition primary size=62958\r\n"));
  assert!(plan
    .partitions
    .contains("gpt attributes=0x8000000000000001"));
  assert!(plan.commands.contains("if defined ambiguous goto failed"));
  assert!(plan
    .commands
    .contains("/Index:6 /ApplyDir:W:\\ /CheckIntegrity /Verify"));
  assert!(!plan.commands.contains("/ForceUnsigned"));
  assert!(!plan.commands.contains("wpeutil reboot"));
  let drivers = plan.commands.find("do call :driver").unwrap();
  let source = plan.commands.find("call :source %%D:").unwrap();
  let partition = plan.commands.find("diskpart /s").unwrap();
  assert!(drivers < source && source < partition);
  let apply = plan.commands.find("dism /Apply-Image").unwrap();
  let boot = plan
    .commands
    .find("bcdboot W:\\Windows /s S: /f UEFI")
    .unwrap();
  let ready = plan
    .commands
    .find("image ready for first-boot provisioning")
    .unwrap();
  assert!(apply < boot && boot < ready);
  for line in plan
    .commands
    .lines()
    .filter(|line| line.starts_with("dism /Image:") || line.starts_with("dism /Apply-Image"))
  {
    assert!(plan
      .commands
      .contains(&format!("{line}\r\nif errorlevel 1 goto failed")));
  }
}

#[test]
fn image_selection_uses_edition_and_architecture_not_position() {
  use engine::machines::windows::image;
  let xml = r#"<WIM><IMAGE INDEX="1"><WINDOWS><ARCH>12</ARCH><EDITIONID>Core</EDITIONID></WINDOWS></IMAGE><IMAGE INDEX="2"><WINDOWS><ARCH>9</ARCH><EDITIONID>Professional</EDITIONID></WINDOWS></IMAGE><IMAGE INDEX="3"><WINDOWS><ARCH>12</ARCH><EDITIONID>Professional</EDITIONID></WINDOWS></IMAGE></WIM>"#;
  assert_eq!(image::professional(xml.as_bytes()).unwrap(), 3);
  for little in [false, true] {
    let mut encoded = if little {
      vec![0xff, 0xfe]
    } else {
      vec![0xfe, 0xff]
    };
    for word in xml.encode_utf16() {
      encoded.extend(if little {
        word.to_le_bytes()
      } else {
        word.to_be_bytes()
      });
    }
    assert_eq!(image::professional(&encoded).unwrap(), 3);
    encoded.push(0);
    assert!(image::professional(&encoded).is_err());
  }
  assert!(image::professional(xml.replace("INDEX=\"3\"", "INDEX=\"1\"").as_bytes()).is_err());
  assert!(
    image::professional(xml.replace("<ARCH>9</ARCH>", "<ARCH>12</ARCH>").as_bytes()).is_err()
  );
  assert!(image::professional(xml.replace("Professional", "Core").as_bytes()).is_err());
  assert!(image::professional(&[0xff, 0xfe, 0, 0xd8]).is_err());
}
