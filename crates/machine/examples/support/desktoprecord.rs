use anyhow::ensure;
use machine::{
  devices::virtio::pci,
  hypervisor::state::group::Checkpoint,
  snapshot::{
    checkpoint,
    manifest::{Binding, Layout},
    package::Package,
  },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
  fs::{File, OpenOptions, Permissions},
  io::{Read, Write},
  os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
  path::Path,
};

type Requests = [Option<u64>; 7];
type Restored = (Checkpoint, [u8; 32], Requests);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
  display: [u8; 32],
  requests: Requests,
}

fn layout() -> Layout {
  Layout {
    cpus: 2,
    banks: [
      (0, 0x4000000),
      (0x4000000, 0x4000000),
      (machine::platform::RAM, 1 << 32),
    ],
    devices: [0, 2, 3, 3, 1, 0, 5],
  }
}

fn binding(configuration: [u8; 32], metadata: &Metadata) -> anyhow::Result<Binding> {
  let mut digest = Sha256::new();
  digest.update(configuration);
  digest.update(serde_json::to_vec(metadata)?);
  // diagnostic identity; production VM record binding remains a separate integration.
  Ok(Binding {
    machine: [1; 16],
    snapshot: [2; 16],
    configuration: digest.finalize().into(),
  })
}

pub fn save(
  root: &Path,
  saved: &Checkpoint,
  display: [u8; 32],
  requests: Requests,
  configuration: [u8; 32],
) -> anyhow::Result<()> {
  ensure!(
    checkpoint::layout(saved)? == layout(),
    "Windows fixture topology changed"
  );
  let metadata = Metadata { display, requests };
  let path = root.join("snapshots");
  std::fs::create_dir(&path)?;
  std::fs::set_permissions(&path, Permissions::from_mode(0o700))?;
  let parent = File::open(&path)?;
  let mut file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(root.join("desktop.json"))?;
  file.write_all(&serde_json::to_vec(&metadata)?)?;
  file.sync_all()?;
  File::open(root)?.sync_all()?;
  let started = std::time::Instant::now();
  drop(checkpoint::save(
    saved,
    &parent,
    binding(configuration, &metadata)?,
    || Ok(()),
    || Ok(()),
  )?);
  eprintln!(
    "Windows checkpoint package serialization completed in {:?}",
    started.elapsed()
  );
  Ok(())
}

pub fn load(
  root: &Path,
  hardware: &[Option<pci::Device>; 7],
  configuration: [u8; 32],
) -> anyhow::Result<Restored> {
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(root.join("desktop.json"))?;
  let info = file.metadata()?;
  ensure!(
    info.is_file()
      && info.len() <= 1024
      && info.nlink() == 1
      && info.mode() & 0o7777 == 0o600
      && info.uid() == unsafe { libc::geteuid() },
    "Windows fixture metadata is not private and bounded"
  );
  let mut bytes = Vec::new();
  file.take(1025).read_to_end(&mut bytes)?;
  let metadata: Metadata = serde_json::from_slice(&bytes)?;
  let parent = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open(root.join("snapshots"))?;
  let package = Package::open(
    &parent,
    binding(configuration, &metadata)?,
    layout(),
    || Ok(()),
  )?;
  let started = std::time::Instant::now();
  let saved = checkpoint::load(&package, hardware, || Ok(()))?;
  eprintln!(
    "Windows checkpoint package decoded in {:?}",
    started.elapsed()
  );
  Ok((saved, metadata.display, metadata.requests))
}
