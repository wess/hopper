use super::platform::Prepared;
use anyhow::ensure;
use std::sync::Arc;

impl Prepared {
  pub fn admit(
    self,
    main: machine::vz::MainThreadMarker,
    owner: &mut super::super::Owner,
    media: Arc<tempfile::TempDir>,
  ) -> anyhow::Result<()> {
    self.admit_owned(main, owner, Some(media))
  }

  pub fn admit_system(
    self,
    main: machine::vz::MainThreadMarker,
    owner: &mut super::super::Owner,
  ) -> anyhow::Result<()> {
    ensure!(self.installed, "Install macOS before system admission");
    self.admit_owned(main, owner, None)
  }

  fn admit_owned(
    mut self,
    main: machine::vz::MainThreadMarker,
    owner: &mut super::super::Owner,
    media: Option<Arc<tempfile::TempDir>>,
  ) -> anyhow::Result<()> {
    let platform = self.publish(main)?;
    let phase = super::deployment::read(&self.target, &self.machine.id)?;
    ensure!(
      phase != Some(super::deployment::Phase::Installing),
      "macOS installation requires recovery; its disk is preserved"
    );
    ensure!(
      phase.is_some() || !super::deployment::written(&self.target)?,
      "Written macOS disk requires recovery; its data is preserved"
    );
    let installed = phase == Some(super::deployment::Phase::Installed);
    ensure!(
      installed == self.installed,
      "macOS deployment changed before admission"
    );
    let boot = machine::vz::mac::Mac {
      cpus: self.machine.resources.cpus as usize,
      memory: u64::from(self.machine.resources.memory_gib) << 30,
      width: 1024,
      height: 768,
      image: self.image.clone(),
      identity: platform.identity,
      auxiliary: self.target.join("auxiliary"),
      disk: self.target.join("disk"),
      network: self.network,
    };
    let mut vm = if installed {
      machine::vz::recover_mac(main, &boot)?
    } else {
      machine::vz::create_mac(main, &boot)?
    };
    machine::vz::retain(&mut vm, Arc::new((self.runtime, media)))?;
    (self.check)()?;
    owner.insert(&self.machine.id, vm)?;
    if let Err(error) = (self.check)() {
      owner.retire(&self.machine.id)?;
      return Err(error);
    }
    Ok(())
  }
}
