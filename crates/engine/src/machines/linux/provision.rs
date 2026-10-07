use crate::machines::{Actor, Machines};
use anyhow::ensure;
use model::{GuestOs, MachineRuntime};
use serde_json::json;
use sha_crypt::{sha512_simple, Sha512Params};
use store::guests::Credentials;
use uuid::Uuid;

pub struct Plan {
  pub user_data: String,
  pub meta_data: String,
}

pub fn accounts() -> Credentials {
  Credentials {
    user_password: format!("Hpr7!{}", Uuid::new_v4().simple()),
    administrator_password: format!("Hpr7!{}", Uuid::new_v4().simple()),
  }
}

pub fn persisted(manager: &Machines, id: &str, actor: Actor) -> anyhow::Result<Credentials> {
  super::super::validate_id(id)?;
  let id = Uuid::parse_str(id)?.to_string();
  let _lock = manager.guard(&id, ".credentials")?;
  let original = manager.machine(&id, actor)?;
  ensure!(
    original.guest == GuestOs::Linux
      && original.runtime == Some(MachineRuntime::Virtualization)
      && original.profile == "ubuntu",
    "Native Ubuntu provisioning requires a native Ubuntu VM"
  );
  let slot = store::guests::open_for(&id, GuestOs::Linux)?;
  let credentials = match store::guests::read(&slot)? {
    Some(saved) => saved,
    None => {
      let generated = accounts();
      store::guests::create(&slot, &generated)?;
      generated
    }
  };
  let current = manager.machine(&id, actor)?;
  ensure!(
    current.guest == original.guest
      && current.runtime == original.runtime
      && current.profile == original.profile
      && (actor != Actor::Agent || current.agent_generation == original.agent_generation),
    "VM provisioning policy changed"
  );
  Ok(credentials)
}

pub fn prepare(id: &str, credentials: &Credentials) -> anyhow::Result<Plan> {
  let id = Uuid::parse_str(id)?;
  ensure!(
    credentials.user_password != credentials.administrator_password
      && [
        &credentials.user_password,
        &credentials.administrator_password
      ]
      .into_iter()
      .all(|value| (8..=128).contains(&value.len()) && !value.chars().any(char::is_control)),
    "Guest credentials are invalid"
  );
  let params =
    Sha512Params::new(100_000).map_err(|_| anyhow::anyhow!("Invalid hash parameters"))?;
  let hash = |password: &str| {
    sha512_simple(password, &params).map_err(|_| anyhow::anyhow!("Cannot hash guest password"))
  };
  let user = hash(&credentials.user_password)?;
  let administrator = hash(&credentials.administrator_password)?;
  let hostname = format!("hopper-{}", &id.simple().to_string()[..12]);
  let configuration = json!({
    "autoinstall": {
      "version": 1,
      "locale": "en_US.UTF-8",
      "keyboard": { "layout": "us" },
      "refresh-installer": { "update": false },
      "source": { "search_drivers": false },
      "identity": {
        "hostname": hostname,
        "username": "hopperadmin",
        "realname": "Hopper administrator",
        "password": administrator
      },
      "storage": { "layout": { "name": "direct", "match": { "path": "/dev/vda" } } },
      "ssh": { "install-server": false, "allow-pw": false },
      "shutdown": "poweroff",
      "user-data": {
        "users": [
          {
            "name": "hopper",
            "gecos": "Hopper",
            "groups": ["users"],
            "shell": "/bin/bash",
            "lock_passwd": false,
            "passwd": user,
            "sudo": false
          }
        ],
        "write_files": [{
          "path": "/etc/gdm3/custom.conf",
          "owner": "root:root",
          "permissions": "0644",
          "content": "[daemon]\nAutomaticLoginEnable=True\nAutomaticLogin=hopper\n[security]\n[xdmcp]\n[chooser]\n[debug]\n"
        }]
      }
    }
  });
  Ok(Plan {
    user_data: format!("#cloud-config\n{}", serde_yaml::to_string(&configuration)?),
    meta_data: serde_yaml::to_string(&json!({
      "instance-id": format!("hopper-{id}"),
      "local-hostname": hostname
    }))?,
  })
}

pub fn tracked(id: &str, credentials: &Credentials, attempt: &str) -> anyhow::Result<Plan> {
  super::progress::Decoder::new(attempt)?;
  let mut plan = prepare(id, credentials)?;
  let mut configuration: serde_json::Value = serde_yaml::from_str(&plan.user_data)?;
  for (key, phase) in [
    ("early-commands", "installing"),
    ("late-commands", "deployed"),
    ("error-commands", "failed"),
  ] {
    configuration["autoinstall"][key] = json!([format!(
      "printf '\\nHOPPER-INSTALL:{attempt}:{phase}\\n' > /dev/hvc0"
    )]);
  }
  plan.user_data = format!("#cloud-config\n{}", serde_yaml::to_string(&configuration)?);
  Ok(plan)
}
