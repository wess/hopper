use anyhow::{ensure, Context};
use fs2::FileExt;
use machine::{
  devices::virtio::{block, console, gpu, input, pci, scsi},
  runtime::{self, variables, Boot},
};
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path, time::Duration};

pub(super) fn prepare(
  config: model::native::Boot,
) -> anyhow::Result<(Boot, [Option<pci::Device>; 7], variables::Variables)> {
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
  let template = read(&config.variables)?;
  let mut boot = Boot {
    firmware: read(&config.firmware)?,
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
  let disk = config
    .disk
    .map(|path| -> anyhow::Result<_> {
      let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
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
    .transpose()?;
  let devices = [
    optical(config.boot_media)?,
    Some(pci::graphics(gpu::create(1024, 768)?)?),
    Some(pci::controller(input::create(input::Kind::Keyboard))?),
    Some(pci::controller(input::create(input::Kind::Tablet))?),
    disk,
    optical(config.installer)?,
    Some(pci::serial(console::create("org.hopper.setup")?)?),
  ];
  let root = Path::new(&config.store);
  let store = if root.exists() {
    variables::open(root)?
  } else {
    variables::create(root, &boot.variables)?
  };
  boot.variables = variables::bytes(&store).to_vec();
  Ok((boot, devices, store))
}

fn read(path: &str) -> anyhow::Result<Vec<u8>> {
  let file = std::fs::File::open(path)?;
  ensure!(
    file.metadata()?.len() <= 0x4000000,
    "Native firmware exceeds its bank"
  );
  let mut bytes = Vec::new();
  file.take(0x4000001).read_to_end(&mut bytes)?;
  ensure!(bytes.len() <= 0x4000000, "Native firmware exceeds its bank");
  Ok(bytes)
}
