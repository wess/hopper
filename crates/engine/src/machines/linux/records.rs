use crate::machines::Machines;
use anyhow::ensure;
use model::{CreateMachine, Machine};

pub fn create(manager: &Machines, request: CreateMachine) -> anyhow::Result<Machine> {
  ensure!(
    request.profile == "ubuntu",
    "Choose a native Ubuntu VM profile"
  );
  super::super::vz::records::create(manager, request)
}
