//! Trusted host/worker hardware reconstruction. Never accept these paths through guest APIs.
use crate::{
  devices::virtio::{block, console, gpu, input, pci, scsi},
  runtime::{self, Boot},
};
use anyhow::{ensure, Context};
use fs2::FileExt;
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path, time::Duration};
mod checkpoint;
pub use checkpoint::checkpoint;

pub fn original(
  config: model::native::Boot,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], [u8; 32])> {
  hardware_using(config, None, &mut authorize)
}

pub fn current(
  config: model::native::Boot,
  disk: Option<block::Disk>,
  mut authorize: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], [u8; 32])> {
  hardware_using(config, Some(disk), &mut authorize)
}

fn hardware_using(
  config: model::native::Boot,
  current: Option<Option<block::Disk>>,
  authorize: &mut impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], [u8; 32])> {
  authorize()?;
  model::native::machine_identity(&config.machine_id).context("Invalid native VM identity")?;
  ensure!(
    config
      .timeout_ms
      .is_none_or(|timeout| timeout <= 86_400_000),
    "Native timeout exceeds one day"
  );
  ensure!(
    config.disk_id.len() == 20 && config.disk_id.is_ascii(),
    "Invalid native disk identity"
  );
  let template = read(&config.variables, authorize)?;
  let boot = Boot {
    firmware: read(&config.firmware, authorize)?,
    variables: template,
    memory: config.memory,
    cpus: config.cpus,
    timeout: config
      .timeout_ms
      .map_or(Duration::MAX, Duration::from_millis),
  };
  runtime::validate(&boot)?;
  let optical = |path: Option<String>| -> anyhow::Result<Option<pci::Device>> {
    path
      .map(|path| pci::optical(scsi::open(Path::new(&path))?))
      .transpose()
  };
  authorize()?;
  let disk = if let Some(current) = current {
    ensure!(
      config.disk.is_some() == current.is_some(),
      "Current disk topology differs from saved VM"
    );
    current.map(pci::create).transpose()?
  } else {
    config
      .disk
      .map(|path| -> anyhow::Result<_> {
        let mut file = OpenOptions::new()
          .read(true)
          .write(true)
          .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
          .open(path)
          .context("Open native guest disk")?;
        file
          .try_lock_exclusive()
          .context("Native guest disk is in use")?;
        ensure!(
          file.metadata()?.is_file(),
          "Native guest disk must be a regular file"
        );
        let mut magic = [0; 4];
        file.read_exact(&mut magic)?;
        ensure!(
          magic != *b"QFI\xfb",
          "Native guest disk requires raw format; preserve the existing image for migration"
        );
        pci::create(block::attach(
          file,
          false,
          config.disk_id.as_bytes().try_into()?,
        )?)
      })
      .transpose()?
  };
  let devices = [
    optical(config.boot_media)?,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    disk,
    optical(config.installer)?,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let configuration = crate::snapshot::configuration::fingerprint(
    &config.machine_id,
    &boot,
    &devices,
    &mut *authorize,
  )?;
  Ok((boot, devices, configuration))
}

fn read(path: &str, authorize: &mut impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<Vec<u8>> {
  authorize()?;
  let mut file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(path)?;
  let metadata = file.metadata()?;
  ensure!(
    metadata.is_file() && metadata.len() <= 0x4000000,
    "Invalid native firmware bank"
  );
  let mut bytes = Vec::new();
  let mut chunk = [0; 65536];
  loop {
    authorize()?;
    let count = file.read(&mut chunk)?;
    authorize()?;
    if count == 0 {
      break;
    }
    ensure!(
      bytes.len() + count <= 0x4000000,
      "Native firmware exceeds its bank"
    );
    bytes.extend_from_slice(&chunk[..count]);
  }
  ensure!(
    bytes.len() as u64 == metadata.len() && file.metadata()?.len() == metadata.len(),
    "Native firmware bank changed"
  );
  authorize()?;
  Ok(bytes)
}
