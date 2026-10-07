use std::process::Command;

pub fn image() -> Vec<u8> {
  let mut bytes = vec![0; 4096];
  bytes[56..60].copy_from_slice(b"ARM\x64");
  bytes
}

pub fn media(root: &std::path::Path) -> std::path::PathBuf {
  let source = root.join("source");
  std::fs::create_dir_all(source.join("casper")).unwrap();
  std::fs::create_dir_all(source.join("boot/grub")).unwrap();
  std::fs::write(source.join("casper/vmlinuz"), image()).unwrap();
  std::fs::write(source.join("casper/initrd"), [0; 128]).unwrap();
  std::fs::write(
    source.join("casper/install-sources.yaml"),
    "- id: ubuntu-desktop\n",
  )
  .unwrap();
  std::fs::write(
    source.join("boot/grub/grub.cfg"),
    format!(
      "linux /casper/vmlinuz\ninitrd /casper/initrd\n#{}\n",
      " ".repeat(1024)
    ),
  )
  .unwrap();
  let media = root.join("installer.iso");
  let status = Command::new("/usr/bin/bsdtar")
    .arg("--format=iso9660")
    .arg("-cf")
    .arg(&media)
    .arg("-C")
    .arg(&source)
    .args(["casper", "boot"])
    .status()
    .unwrap();
  assert!(status.success());
  media
}
