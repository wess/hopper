use super::{native::assets, Actor, Machines};
use anyhow::ensure;
use model::Machine;
use std::io::Read;

impl Machines {
  pub(crate) fn records(&self, actor: Actor) -> anyhow::Result<Vec<Machine>> {
    let folder = self.root.join("records");
    if !folder.try_exists()? {
      return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(folder)? {
      let path = entry?.path();
      if path.extension().is_none_or(|extension| extension != "json") {
        continue;
      }
      let file = assets::regular(&path, 1024 * 1024)?;
      let machine: Machine = serde_json::from_reader(file.take(1024 * 1024 + 1))?;
      super::validate_id(&machine.id)?;
      ensure!(
        path.file_stem().and_then(|stem| stem.to_str()) == Some(machine.id.as_str()),
        "VM record does not match its filename"
      );
      if actor == Actor::Agent && !machine.agent_access {
        continue;
      }
      records.push(machine);
    }
    Ok(records)
  }
}
