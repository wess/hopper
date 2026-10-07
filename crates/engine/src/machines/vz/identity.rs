use super::files;
use anyhow::ensure;
use model::Machine;
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Identity {
  pub id: String,
  pub version: u32,
  pub guest: model::GuestOs,
  pub disk_gib: u32,
  pub identity: Vec<u8>,
}

pub(super) fn validate(target: &std::path::Path, machine: &Machine) -> anyhow::Result<Identity> {
  files::directory(target)?;
  let identity: Identity =
    serde_json::from_reader(files::read(&target.join("identity"), 65536)?.take(65537))?;
  ensure!(
    identity.id == machine.id
      && identity.version == 1
      && identity.guest == model::GuestOs::Linux
      && identity.disk_gib == machine.resources.disk_gib
      && identity.identity.len() <= 4096,
    "VZ identity or disk capacity does not match the VM record"
  );
  let mut disk = files::read(&target.join("disk"), 2048 << 30)?;
  ensure!(
    disk.metadata()?.len() == u64::from(machine.resources.disk_gib) << 30,
    "VZ disk capacity changed; preserve it for recovery"
  );
  let mut magic = [0; 4];
  disk.read_exact(&mut magic)?;
  ensure!(
    &magic != b"QFI\xfb",
    "Previous disk format requires migration"
  );
  files::read(&target.join("variables"), 16 << 20)?;
  Ok(identity)
}
