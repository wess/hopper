use anyhow::ensure;
use engine::machines::windows::{
  deploy, provision,
  setup::{self, Input, Tools},
};
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let args: Vec<_> = std::env::args().skip(1).collect();
  ensure!(args.len() == 6, "Provide installer ISO, extracted drivers root, license, WIM helper, image helper and a new output directory");
  let mut directory = std::fs::DirBuilder::new();
  #[cfg(unix)]
  {
    use std::os::unix::fs::DirBuilderExt;
    directory.mode(0o700);
  }
  directory.create(&args[5])?;
  let output = Path::new(&args[5]).join("deployment.iso");
  let id = uuid::Uuid::new_v4().to_string();
  let plan = deploy::prepare(&deploy::Layout {
    disk_gib: 64,
    image_index: 3,
    recovery_mib: 2048,
    recovery_image_bytes: 900 * 1024 * 1024,
  })?;
  let provision = provision::prepare(&id, &provision::accounts())?;
  setup::build(
    &Input {
      vm_id: &id,
      installer: Path::new(&args[0]),
      drivers: Path::new(&args[1]),
      license: Path::new(&args[2]),
      output: &output,
      plan: &plan,
      provision: &provision,
    },
    &Tools {
      archive: "/usr/bin/tar".into(),
      wim: args[3].clone().into(),
      image: args[4].clone().into(),
    },
    |phase| println!("{phase:?}"),
  )
  .await?;
  println!("Built private setup media; no VM or OS installation was started");
  Ok(())
}
