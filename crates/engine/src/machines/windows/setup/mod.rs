//! Private per-VM setup media. Publication never replaces an existing image.

pub mod archive;
pub mod files;
pub(crate) mod process;

use super::{deploy::Plan, provision::Provision};
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{
  io::Read,
  path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Tools {
  pub archive: PathBuf,
  pub wim: PathBuf,
  pub image: PathBuf,
}

pub struct Input<'a> {
  pub vm_id: &'a str,
  pub installer: &'a Path,
  pub drivers: &'a Path,
  pub license: &'a Path,
  pub output: &'a Path,
  pub plan: &'a Plan,
  pub provision: &'a Provision,
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
  VerifyingSource,
  Extracting,
  Drivers,
  Updating,
  Building,
}

const BOOTSTRAP: &str = "@echo off\r\ntitle Hopper native Windows deployment\r\nwpeinit\r\nif errorlevel 1 goto failed\r\ndrvload X:\\hopper\\vioserial\\vioser.inf\r\nif errorlevel 1 goto failed\r\nfor /L %%I in (1,1,10) do (\r\n  cmd /c exit 0\r\n  >\\\\.\\org.hopper.setup echo HOPPERSETUP/1 phase 0\r\n  if not errorlevel 1 goto connected\r\n  ping 127.0.0.1 -n 2 >nul\r\n)\r\ngoto failed\r\n:connected\r\nset HOPPER_SETUP_PORT=\\\\.\\org.hopper.setup\r\ncall X:\\hopper\\deploy.cmd\r\nif errorlevel 1 goto failed\r\necho Hopper image prepared. Stop the VM and boot from its new disk.\r\ngoto finished\r\n:failed\r\necho Hopper deployment stopped. The VM has not been marked ready.\r\n:finished\r\ncmd /k\r\n";

pub async fn build(
  input: &Input<'_>,
  tools: &Tools,
  mut progress: impl FnMut(Phase),
) -> anyhow::Result<()> {
  ensure!(
    input.installer.is_absolute() && input.output.is_absolute(),
    "Setup paths must be absolute"
  );
  let parent = input
    .output
    .parent()
    .context("Setup output has no parent directory")?;
  let info = std::fs::symlink_metadata(parent)?;
  ensure!(
    info.is_dir(),
    "Setup output directory must not be a symlink"
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
      info.permissions().mode() & 0o077 == 0,
      "Setup output directory must be private"
    );
  }
  ensure!(
    !input.output.try_exists()?,
    "Setup image already exists; preserve it for recovery"
  );
  crate::machines::validate_id(input.vm_id)?;
  ensure!(
    uuid::Uuid::parse_str(input.vm_id)?.to_string() == input.vm_id,
    "Setup identity must be canonical"
  );
  ensure!(
    input
      .output
      .file_name()
      .is_some_and(|name| name == "deployment.iso"),
    "Unexpected setup output filename"
  );
  ensure!(
    std::fs::read_dir(parent)?.next().is_none(),
    "Setup directory already contains data; preserve it for recovery"
  );
  let temporary = tempfile::tempdir_in(parent.parent().context("Setup directory has no parent")?)?;
  let root = temporary.path().canonicalize()?;
  let text = root.to_str().context("Setup path is not UTF-8")?;
  ensure!(
    !text.contains(['"', '\r', '\n']),
    "Setup path cannot be represented in the WIM command language"
  );
  let payload = root.join("payload");
  let media = root.join("media");
  progress(Phase::VerifyingSource);
  let installer_hash = files::digest(input.installer, 12 * 1024 * 1024 * 1024).await?;
  std::fs::create_dir(&payload)?;
  std::fs::create_dir(&media)?;
  for (name, content) in [
    ("partitions.txt", &input.plan.partitions),
    ("deploy.cmd", &input.plan.commands),
    ("unattend.xml", &input.provision.answer),
    ("hopperspecialize.ps1", &input.provision.specialize),
    ("hopperfirstlogon.ps1", &input.provision.firstlogon),
  ] {
    ensure!(
      (1..=256 * 1024).contains(&content.len()),
      "Invalid deployment payload size"
    );
    files::write(&payload.join(name), content.as_bytes())?;
  }
  progress(Phase::Drivers);
  let hashes = files::drivers(input.drivers, input.license, &payload)?;
  files::write(&payload.join("drivers.cmd"), BOOTSTRAP.as_bytes())?;
  let shell = root.join("winpeshl.ini");
  files::write(
    &shell,
    b"[LaunchApps]\r\n%SYSTEMROOT%\\System32\\cmd.exe, /c X:\\hopper\\drivers.cmd\r\n",
  )?;
  progress(Phase::Extracting);
  let media_bytes = archive::extract(&tools.archive, input.installer, &media).await?;
  progress(Phase::Updating);
  let boot = media.join("sources/boot.wim");
  let commands = format!(
    "add \"{}\" /hopper\nadd \"{}\" /Windows/System32/winpeshl.ini\n",
    payload.display(),
    shell.display()
  );
  process::run(
    &tools.wim,
    &["update".into(), path(&boot)?, "2".into()],
    Some(commands.as_bytes()),
    None,
    1024 * 1024,
  )
  .await?;
  ensure!(
    fs2::available_space(parent)? > media_bytes + 256 * 1024 * 1024,
    "Setup image construction needs more free disk space"
  );
  progress(Phase::Building);
  let publication = root.join("publication");
  std::fs::create_dir(&publication)?;
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&publication, std::fs::Permissions::from_mode(0o700))?;
  }
  let image = publication.join("deployment.iso");
  process::run(
    &tools.image,
    &[
      "-udf".into(),
      "-iso-level".into(),
      "3".into(),
      "-V".into(),
      "HOPPERSETUP".into(),
      "-eltorito-platform".into(),
      "efi".into(),
      "-b".into(),
      "efi/microsoft/boot/efisys.bin".into(),
      "-no-emul-boot".into(),
      "-o".into(),
      path(&image)?,
      path(&media)?,
    ],
    None,
    None,
    1024 * 1024,
  )
  .await?;
  let mut file = std::fs::File::open(&image)?;
  let size = file.metadata()?.len();
  ensure!(
    (1..=2 * 1024 * 1024 * 1024).contains(&size),
    "Invalid setup image size"
  );
  let mut digest = Sha256::new();
  let mut buffer = vec![0; 64 * 1024];
  loop {
    let read = file.read(&mut buffer)?;
    if read == 0 {
      break;
    }
    digest.update(&buffer[..read]);
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o600))?;
  }
  file.sync_all()?;
  progress(Phase::VerifyingSource);
  ensure!(
    files::digest(input.installer, 12 * 1024 * 1024 * 1024).await? == installer_hash,
    "Installer changed during setup construction"
  );
  let manifest = serde_json::to_vec(&serde_json::json!({
    "vmId": input.vm_id, "size": size, "sha256": format!("{:x}", digest.finalize()), "driverSha256": hashes,
    "containsGuestCredentials": true, "detachBeforeFirstBoot": true,
    "installerSha256": installer_hash, "planSha256": plan_hash(input.plan),
  }))?;
  files::write(&publication.join("deployment.json"), &manifest)?;
  std::fs::File::open(&publication)?.sync_all()?;
  // rename can replace an empty directory, but cannot overwrite an existing bundle.
  std::fs::rename(&publication, parent).context("Publish complete private setup bundle")?;
  std::fs::File::open(parent.parent().unwrap())?.sync_all()?;
  Ok(())
}

pub(crate) async fn verify_source(
  output: &Path,
  installer: &Path,
  plan: &Plan,
) -> anyhow::Result<()> {
  let bytes = files::read(&output.with_extension("json"), 1024 * 1024)?;
  let metadata: serde_json::Value = serde_json::from_slice(&bytes)?;
  ensure!(
    metadata["planSha256"].as_str() == Some(plan_hash(plan).as_str()),
    "Existing setup bundle uses a different deployment layout; preserve it for recovery"
  );
  let hash = files::digest(installer, 12 * 1024 * 1024 * 1024).await?;
  ensure!(
    metadata["installerSha256"].as_str() == Some(hash.as_str()),
    "Existing setup bundle uses different installation media; preserve it for recovery"
  );
  Ok(())
}

fn plan_hash(plan: &Plan) -> String {
  let mut hash = Sha256::new();
  hash.update((plan.partitions.len() as u64).to_le_bytes());
  hash.update(&plan.partitions);
  hash.update(&plan.commands);
  format!("{:x}", hash.finalize())
}

fn path(path: &Path) -> anyhow::Result<String> {
  Ok(path.to_str().context("Setup path is not UTF-8")?.to_owned())
}
