use super::Plan;
use anyhow::ensure;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;

const UNIT: &str = include_str!("../../../../../guest/assets/hopperguest.service");
const EXECUTABLE: &str = "/usr/libexec/hopperguest";
const SERVICE: &str = "/etc/systemd/system/hopperguest.service";
const NOTICES: &str = "/usr/share/doc/hopperguest/licenses.html";
const RULE: &str = "/etc/udev/rules.d/99hopperboot.rules";
const RULE_CONTENT: &str = include_str!("../../../../../guest/assets/hopperboot.rules");

/// install public tools only; boot authentication material has a separate delivery path.
pub fn with_tools(plan: &Plan, executable: &[u8], notices: &[u8]) -> anyhow::Result<Plan> {
  ensure!(
    executable.len() >= 64
      && executable.len() <= 1 << 20
      && &executable[..7] == b"\x7fELF\x02\x01\x01"
      && matches!(
        u16::from_le_bytes(executable[16..18].try_into().unwrap()),
        2 | 3
      )
      && u16::from_le_bytes(executable[18..20].try_into().unwrap()) == 183
      && u32::from_le_bytes(executable[20..24].try_into().unwrap()) == 1,
    "Guest tools require a bounded ARM64 Linux ELF executable"
  );
  ensure!(
    plan.user_data.starts_with("#cloud-config\n")
      && plan.user_data.len() <= 65536
      && !plan.user_data.contains('\0')
      && (1..=4096).contains(&plan.meta_data.len())
      && !plan.meta_data.contains('\0'),
    "Base guest provisioning plan exceeds bounds"
  );
  ensure!(
    (1..=4 << 20).contains(&notices.len())
      && !notices.contains(&0)
      && std::str::from_utf8(notices).is_ok(),
    "Guest license notices exceed bounds or contain invalid text"
  );
  let mut configuration: serde_json::Value = serde_yaml::from_str(&plan.user_data)?;
  let install = configuration
    .get_mut("autoinstall")
    .and_then(|value| value.as_object_mut())
    .ok_or_else(|| anyhow::anyhow!("Guest installer configuration is missing"))?;
  ensure!(
    install.get("version").and_then(|value| value.as_u64()) == Some(1),
    "Guest installer configuration version is invalid"
  );
  let guest = install
    .get_mut("user-data")
    .and_then(|value| value.as_object_mut())
    .ok_or_else(|| anyhow::anyhow!("Guest provisioning data is missing"))?;
  let files = guest
    .get_mut("write_files")
    .and_then(|value| value.as_array_mut())
    .ok_or_else(|| anyhow::anyhow!("Guest file configuration is missing"))?;
  ensure!(
    files.len() <= 64
      && !files.iter().any(|file| {
        matches!(
          file["path"].as_str(),
          Some(EXECUTABLE | SERVICE | RULE | NOTICES | "/run/hopper/boot")
        )
      }),
    "Guest tool or credential paths already exist in the provisioning plan"
  );
  files.extend([
    json!({ "path": NOTICES, "owner": "root:root", "permissions": "0644",
      "encoding": "b64", "content": STANDARD.encode(notices) }),
    json!({ "path": EXECUTABLE, "owner": "root:root", "permissions": "0755",
      "encoding": "b64", "content": STANDARD.encode(executable) }),
    json!({ "path": SERVICE, "owner": "root:root", "permissions": "0644", "content": UNIT }),
    json!({ "path": RULE, "owner": "root:root", "permissions": "0644", "content": RULE_CONTENT }),
  ]);
  let commands = guest
    .get_mut("runcmd")
    .and_then(|value| value.as_array_mut())
    .ok_or_else(|| anyhow::anyhow!("Guest startup commands are missing"))?;
  ensure!(commands.len() <= 64, "Guest startup commands exceed bounds");
  commands.extend([
    json!(["systemctl", "daemon-reload"]),
    json!(["systemctl", "enable", "--now", "hopperguest.service"]),
  ]);
  let plan = Plan {
    user_data: format!("#cloud-config\n{}", serde_yaml::to_string(&configuration)?),
    meta_data: plan.meta_data.clone(),
  };
  ensure!(
    plan.user_data.len() <= super::super::seed::USER_DATA_LIMIT,
    "Guest tool provisioning exceeds bounds"
  );
  Ok(plan)
}
