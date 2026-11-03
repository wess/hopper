#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use base64::{engine::general_purpose::STANDARD, Engine};
use engine::machines::linux::{
  provision::{self, Plan},
  seed,
};
use serde_json::{json, Value};
use std::process::Command;

fn executable(size: usize) -> Vec<u8> {
  let mut bytes = vec![0; size];
  bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
  bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
  bytes[18..20].copy_from_slice(&183u16.to_le_bytes());
  bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
  bytes
}

fn plan() -> Plan {
  Plan {
    user_data: format!(
      "#cloud-config\n{}",
      serde_yaml::to_string(&json!({
        "autoinstall": {"version": 1, "user-data": {
          "users": [{"name": "hopper", "sudo": false}],
          "write_files": [{"path": "/existing", "content": "existing"}],
          "runcmd": [["existing", "literal; argument"]]
        }}
      }))
      .unwrap()
    ),
    meta_data: "instance-id: hopper-test\n".into(),
  }
}

#[test]
fn public_tools_survive_nocloud_iso_and_preserve_accounts_and_existing_commands() {
  let original = plan();
  let binary = executable(1 << 20);
  let plan = provision::with_tools(&original, &binary, b"license notices").unwrap();
  let image = seed::image(&plan).unwrap();
  assert!(image.len() < 3 << 20);
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("seed.iso");
  std::fs::write(&path, image).unwrap();
  let output = Command::new("/usr/bin/bsdtar")
    .arg("-xOf")
    .arg(path)
    .arg("user-data")
    .output()
    .unwrap();
  assert!(output.status.success());
  let config: Value = serde_yaml::from_slice(&output.stdout).unwrap();
  let guest = &config["autoinstall"]["user-data"];
  assert_eq!(guest["users"], json!([{"name": "hopper", "sudo": false}]));
  assert_eq!(guest["runcmd"][0], json!(["existing", "literal; argument"]));
  assert_eq!(
    guest["runcmd"][2],
    json!(["systemctl", "enable", "--now", "hopperguest.service"])
  );
  let files = guest["write_files"].as_array().unwrap();
  let binary_file = files
    .iter()
    .find(|file| file["path"] == "/usr/libexec/hopperguest")
    .unwrap();
  assert_eq!(binary_file["owner"], "root:root");
  assert_eq!(binary_file["permissions"], "0755");
  assert_eq!(binary_file["encoding"], "b64");
  assert_eq!(
    STANDARD
      .decode(binary_file["content"].as_str().unwrap())
      .unwrap(),
    binary
  );
  let unit = files
    .iter()
    .find(|file| file["path"] == "/etc/systemd/system/hopperguest.service")
    .unwrap();
  assert_eq!(unit["permissions"], "0644");
  assert!(unit["content"]
    .as_str()
    .unwrap()
    .contains("ExecStartPre=/usr/libexec/hopperguest --bootstrap\n"));
  let rule = files
    .iter()
    .find(|file| file["path"] == "/etc/udev/rules.d/99hopperboot.rules")
    .unwrap();
  assert_eq!(rule["permissions"], "0644");
  assert!(rule["content"].as_str().unwrap().contains("MODE=\"0600\""));
  assert!(files.iter().all(|file| file["path"] != "/run/hopper/boot"));
  assert_eq!(plan.meta_data, original.meta_data);
  assert!(provision::with_tools(&plan, &binary, b"license notices").is_err());
}

#[test]
fn wrong_architecture_oversized_tools_and_conflicting_paths_reject() {
  let original = plan();
  for bytes in [
    vec![],
    executable((1 << 20) + 1),
    {
      let mut bytes = executable(64);
      bytes[18] = 62;
      bytes
    },
    {
      let mut bytes = executable(64);
      bytes[5] = 2;
      bytes
    },
    {
      let mut bytes = executable(64);
      bytes[16] = 1;
      bytes
    },
  ] {
    assert!(provision::with_tools(&original, &bytes, b"license notices").is_err());
  }
  for path in [
    "/usr/libexec/hopperguest",
    "/etc/systemd/system/hopperguest.service",
    "/run/hopper/boot",
    "/usr/share/doc/hopperguest/licenses.html",
    "/etc/udev/rules.d/99hopperboot.rules",
  ] {
    let mut configuration: Value = serde_yaml::from_str(&original.user_data).unwrap();
    configuration["autoinstall"]["user-data"]["write_files"][0]["path"] = path.into();
    let conflicting = Plan {
      user_data: format!(
        "#cloud-config\n{}",
        serde_yaml::to_string(&configuration).unwrap()
      ),
      meta_data: original.meta_data.clone(),
    };
    assert!(provision::with_tools(&conflicting, &executable(64), b"license notices").is_err());
  }
  for config in [
    "[]",
    "null",
    "plain",
    "autoinstall: []",
    "autoinstall: {version: 2, user-data: {}}",
  ] {
    let invalid = Plan {
      user_data: format!("#cloud-config\n{config}"),
      meta_data: original.meta_data.clone(),
    };
    assert!(provision::with_tools(&invalid, &executable(64), b"license notices").is_err());
  }
}

#[test]
fn notices_are_installed_verbatim_and_bounded_with_the_executable() {
  let original = plan();
  let binary = executable(1 << 20);
  let mut notices = vec![b'a'; 4 << 20];
  notices[..6].copy_from_slice("é<&>\n".as_bytes());
  let plan = provision::with_tools(&original, &binary, &notices).unwrap();
  let config: Value = serde_yaml::from_str(&plan.user_data).unwrap();
  let file = config["autoinstall"]["user-data"]["write_files"]
    .as_array()
    .unwrap()
    .iter()
    .find(|file| file["path"] == "/usr/share/doc/hopperguest/licenses.html")
    .unwrap();
  assert_eq!(file["permissions"], "0644");
  assert_eq!(file["owner"], "root:root");
  assert_eq!(
    STANDARD.decode(file["content"].as_str().unwrap()).unwrap(),
    notices
  );
  assert!(seed::image(&plan).unwrap().len() < (8 << 20) + 65536);
  for notices in [vec![], vec![0xff], vec![0], vec![b'a'; (4 << 20) + 1]] {
    assert!(provision::with_tools(&original, &binary, &notices).is_err());
  }
}
