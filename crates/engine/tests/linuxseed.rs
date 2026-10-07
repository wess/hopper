#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use engine::machines::linux::{provision, seed};
use serde_json::Value;
use sha_crypt::sha512_check;
use std::process::Command;

#[test]
fn guest_accounts_have_distinct_hashes_and_explicit_roles_without_plaintext() {
  let accounts = provision::accounts();
  let id = "8197e0f0-0603-43e9-a817-eaf7ab0327af";
  let plan = provision::prepare(id, &accounts).unwrap();
  assert!(!plan.user_data.contains(&accounts.user_password));
  assert!(!plan.user_data.contains(&accounts.administrator_password));
  let config: Value = serde_yaml::from_str(&plan.user_data).unwrap();
  let install = &config["autoinstall"];
  let user = &install["user-data"]["users"][0];
  assert_eq!(user["name"], "hopper");
  assert_eq!(user["sudo"], false);
  assert_eq!(user["groups"], serde_json::json!(["users"]));
  assert_eq!(install["identity"]["username"], "hopperadmin");
  assert!(sha512_check(&accounts.user_password, user["passwd"].as_str().unwrap()).is_ok());
  assert!(sha512_check(
    &accounts.administrator_password,
    install["identity"]["password"].as_str().unwrap()
  )
  .is_ok());
  assert!(sha512_check(
    &accounts.user_password,
    install["identity"]["password"].as_str().unwrap()
  )
  .is_err());
  assert_eq!(install["storage"]["layout"]["match"]["path"], "/dev/vda");
  assert_eq!(install["shutdown"], "poweroff");
  assert_eq!(install["ssh"]["install-server"], false);
  let metadata: Value = serde_yaml::from_str(&plan.meta_data).unwrap();
  assert_eq!(metadata["instance-id"], format!("hopper-{id}"));
  assert!(provision::prepare("../registry.auths", &accounts).is_err());
  let mut same = provision::accounts();
  same.administrator_password = same.user_password.clone();
  assert!(provision::prepare(id, &same).is_err());
}

#[test]
fn independent_iso_reader_recovers_exact_nocloud_names_and_multisector_data() {
  let plan = provision::Plan {
    user_data: format!("#cloud-config\n{}", "x".repeat(65000)),
    meta_data: "instance-id: hopper-owned-probe\n".into(),
  };
  let image = seed::image(&plan).unwrap();
  assert!(image.len() < 128 << 10);
  assert_eq!(&image[16 * 2048 + 40..16 * 2048 + 46], b"CIDATA");
  let root = tempfile::tempdir().unwrap();
  let path = root.path().join("seed.iso");
  std::fs::write(&path, image).unwrap();
  for (name, expected) in [("user-data", plan.user_data), ("meta-data", plan.meta_data)] {
    let output = Command::new("/usr/bin/bsdtar")
      .arg("-xOf")
      .arg(&path)
      .arg(name)
      .output()
      .unwrap();
    assert!(
      output.status.success(),
      "Independent ISO reader rejected seed"
    );
    assert!(output.stdout == expected.as_bytes());
  }
}

#[test]
fn seed_rejects_invalid_or_oversized_configuration() {
  for (user_data, meta_data) in [
    ("autoinstall: {}".into(), "instance-id: owned".into()),
    (
      format!("#cloud-config\n{}", "x".repeat(65536)),
      "owned".into(),
    ),
    ("#cloud-config\nvalid".into(), "x".repeat(4097)),
    ("#cloud-config\n\0".into(), "owned".into()),
    ("#cloud-config\nvalid".into(), String::new()),
  ] {
    assert!(seed::image(&provision::Plan {
      user_data,
      meta_data
    })
    .is_err());
  }
}
