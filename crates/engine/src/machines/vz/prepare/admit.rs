use super::super::{
  files,
  identity::{validate, Identity},
  Installation, Owner, Service,
};
use super::{Admission, Prepared};
use machine::vz::{self, MainThreadMarker};
use std::{fs::File, io::Write, sync::Arc};

impl Prepared {
  pub fn admit(mut self, main: MainThreadMarker, owner: &mut Owner) -> anyhow::Result<Admission> {
    (self.check)()?;
    if let Some(temporary) = self.temporary.take() {
      vz::create_variables(&temporary.path().join("variables"))?;
      File::open(temporary.path().join("variables"))?.sync_all()?;
      let identity = Identity {
        id: self.machine.id.clone(),
        version: 1,
        guest: self.machine.guest,
        disk_gib: self.machine.resources.disk_gib,
        identity: vz::identity(),
      };
      let mut file = files::write(&temporary.path().join("identity"))?;
      serde_json::to_writer(&mut file, &identity)?;
      file.flush()?;
      file.sync_all()?;
      (self.check)()?;
      files::publish(temporary, &self.target)?;
    }
    let identity = validate(&self.target, &self.machine)?;
    if self.installer.is_none() {
      (self.check)()?;
      crate::machines::linux::progress::system(&self.target)?;
    }
    let observation = self
      .attempt
      .as_deref()
      .map(|attempt| {
        crate::machines::linux::observe::start_owned(&self.target, attempt, self.runtime.clone())
      })
      .transpose()?;
    let installer = self.installer.is_some();
    let boot = vz::Linux {
      cpus: self.machine.resources.cpus as usize,
      memory: u64::from(self.machine.resources.memory_gib) << 30,
      width: 1024,
      height: 768,
      identity: identity.identity,
      boot: vz::Boot::Efi {
        variables: self.target.join("variables"),
      },
      disk: self.target.join("disk"),
      installer: self.installer,
      seed: self
        .seed
        .as_ref()
        .map(|directory| directory.path().join("seed")),
      network: Some(self.network),
      shares: self.shares,
      speakers: self.speakers,
      console: observation
        .as_ref()
        .map(|(output, _)| output.try_clone())
        .transpose()?,
    };
    let mut vm = vz::create(main, &boot)?;
    if installer {
      vz::restrict_installer_restart(&mut vm)?;
    }
    vz::retain(
      &mut vm,
      Arc::new((self.runtime, self.seed, self.media, observation)),
    )?;
    (self.check)()?;
    owner.insert(&self.machine.id, vm)?;
    let installation = self
      .attempt
      .map(|attempt| {
        Installation::new(
          Service::new(self.manager, self.client.clone()),
          self.machine.clone(),
          self.actor,
          attempt,
          owner.inspect(&self.machine.id)?.generation,
          self.intent,
        )
      })
      .transpose()?;
    Ok(Admission {
      id: self.machine.id,
      check: self.check,
      client: self.client,
      installation,
    })
  }
}
