use serde_json::{json, Value};

pub const SCRIPT: &str = include_str!("sharing/mount.sh");
pub const SERVICE: &str = include_str!("sharing/mount.service");

pub(super) fn files() -> Vec<Value> {
  vec![
    json!({ "path": "/usr/lib/hopper/shares", "owner": "root:root",
      "permissions": "0755", "content": SCRIPT }),
    json!({ "path": "/etc/systemd/system/hopper-shares.service", "owner": "root:root",
      "permissions": "0644", "content": SERVICE }),
    json!({ "path": "/etc/modules-load.d/hopper.conf", "owner": "root:root",
      "permissions": "0644", "content": "virtiofs\n" }),
  ]
}

pub(super) fn commands() -> Value {
  json!([
    ["modprobe", "virtiofs"],
    ["systemctl", "daemon-reload"],
    ["systemctl", "enable", "--now", "hopper-shares.service"],
    ["ln", "-sT", "/run/hopper-shares", "/home/hopper/Shared"],
  ])
}
