use anyhow::{ensure, Context};
use engine::machines::windows::{deploy, image, provision};
use std::{io::Write, path::Path};

fn main() -> anyhow::Result<()> {
  let args: Vec<_> = std::env::args().skip(1).collect();
  ensure!(args.len() == 5,
    "Provide WIM metadata XML, a new output directory, disk GiB, recovery MiB and recovery image bytes");
  let layout = deploy::Layout {
    disk_gib: args[2].parse()?,
    image_index: image::professional(&std::fs::read(&args[0])?)?,
    recovery_mib: args[3].parse()?,
    recovery_image_bytes: args[4].parse()?,
  };
  let plan = deploy::prepare(&layout)?;
  let accounts = provision::accounts();
  let provisioning = provision::prepare(&uuid::Uuid::new_v4().to_string(), &accounts)?;
  let output = Path::new(&args[1]);
  let mut directory = std::fs::DirBuilder::new();
  #[cfg(unix)]
  {
    use std::os::unix::fs::DirBuilderExt;
    directory.mode(0o700);
  }
  directory
    .create(output)
    .context("Deployment output must be a new directory")?;
  for (name, bytes) in [
    ("partitions.txt", plan.partitions),
    ("deploy.cmd", plan.commands),
    ("unattend.xml", provisioning.answer),
    ("hopperspecialize.ps1", provisioning.specialize),
    ("hopperfirstlogon.ps1", provisioning.firstlogon),
  ] {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
      use std::os::unix::fs::OpenOptionsExt;
      options.mode(0o600);
    }
    let mut file = options.open(output.join(name))?;
    file.write_all(bytes.as_bytes())?;
    file.sync_all()?;
  }
  println!(
    "Prepared image {} deployment scripts; no VM or installation was started",
    layout.image_index
  );
  Ok(())
}
