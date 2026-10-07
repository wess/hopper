use engine::machines::windows::{
  deploy, provision,
  setup::{self, Input, Tools},
};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use tempfile::TempDir;

pub const ID: &str = "8197e0f0-0603-43e9-a817-eaf7ab0327af";

pub struct Fixture {
  pub root: TempDir,
  pub output: PathBuf,
  installer: PathBuf,
  drivers: PathBuf,
  license: PathBuf,
  plan: deploy::Plan,
  provision: provision::Provision,
  pub tools: Tools,
}

impl Fixture {
  pub fn new(mode: &str) -> Self {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("setup");
    std::fs::create_dir(&output).unwrap();
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = output.join("deployment.iso");
    let installer = root.path().join("installer.iso");
    std::fs::write(&installer, b"source image must remain unchanged").unwrap();
    let drivers = root.path().join("drivers");
    for (driver, names) in [
      ("viostor", vec!["viostor.inf", "viostor.cat", "viostor.sys"]),
      ("vioscsi", vec!["vioscsi.inf", "vioscsi.cat", "vioscsi.sys"]),
      (
        "vioinput",
        vec![
          "vioinput.inf",
          "vioinput.cat",
          "vioinput.sys",
          "viohidkmdf.sys",
        ],
      ),
      ("vioserial", vec!["vioser.inf", "vioser.cat", "vioser.sys"]),
    ] {
      let folder = drivers.join(driver).join("w11/ARM64");
      std::fs::create_dir_all(&folder).unwrap();
      for name in names {
        let bytes = if name.ends_with(".sys") {
          let mut bytes = vec![0; 70];
          bytes[..2].copy_from_slice(b"MZ");
          bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
          bytes[64..68].copy_from_slice(b"PE\0\0");
          bytes[68..].copy_from_slice(&0xaa64u16.to_le_bytes());
          bytes
        } else {
          b"NTARM64".to_vec()
        };
        std::fs::write(folder.join(name), bytes).unwrap();
      }
    }
    let license = root.path().join("license");
    std::fs::write(&license, b"synthetic fixture license").unwrap();
    for name in ["archive", "wim", "image"] {
      let path = root.path().join(name);
      std::fs::write(&path, include_str!("tool.py")).unwrap();
      std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(root.path().join("mode"), mode).unwrap();
    let tools = Tools {
      archive: root.path().join("archive"),
      wim: root.path().join("wim"),
      image: root.path().join("image"),
    };
    let plan = deploy::prepare(&deploy::Layout {
      disk_gib: 64,
      image_index: 3,
      recovery_mib: 2048,
      recovery_image_bytes: 900 * 1024 * 1024,
    })
    .unwrap();
    let provision = provision::prepare(ID, &provision::accounts()).unwrap();
    Self {
      root,
      output,
      installer,
      drivers,
      license,
      plan,
      provision,
      tools,
    }
  }

  pub fn input(&self) -> Input<'_> {
    Input {
      vm_id: ID,
      installer: &self.installer,
      drivers: &self.drivers,
      license: &self.license,
      output: &self.output,
      plan: &self.plan,
      provision: &self.provision,
    }
  }

  pub async fn build(&self) -> anyhow::Result<()> {
    setup::build(&self.input(), &self.tools, |_| {}).await
  }
}
