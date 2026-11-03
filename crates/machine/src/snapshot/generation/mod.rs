//! recovery generations; host selection/rollback require a separate durable transaction.
//! failures preserve stages. This layer never switches a VM or authorizes replay after execution.

use super::{
  checkpoint,
  package::{fs, Package},
};
use crate::{
  devices::virtio::pci,
  hypervisor::state::group::Checkpoint,
  runtime::{
    recovery::Disks,
    variables::{self, Variables},
  },
};
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{
  ffi::CString,
  fs::File,
  io::{Read, Write},
  os::fd::AsRawFd,
};

mod attempt;
mod ownership;
mod system;
pub use system::System;
mod consume;

pub struct Stage {
  parent: File,
  name: CString,
  generation: [u8; 16],
  ready: Generation,
}

pub struct Generation {
  receipt: Vec<u8>,
  directory: File,
  directories: [Option<File>; 7],
  variables: File,
  checkpoint: Checkpoint,
  store: Variables,
  disks: Disks,
}

fn name(id: [u8; 16]) -> CString {
  fs::name(
    &id
      .iter()
      .map(|byte| format!("{byte:02x}"))
      .collect::<String>(),
  )
}

fn receipt(package: &Package, generation: [u8; 16]) -> anyhow::Result<Vec<u8>> {
  ensure!(generation != [0; 16], "Restore generation is unset");
  let mut bytes = Vec::from(b"HGR1");
  bytes.extend_from_slice(&generation);
  bytes.extend_from_slice(&package.manifest_bytes());
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  Ok(bytes)
}

fn check_receipt(directory: &File, expected: &[u8]) -> anyhow::Result<()> {
  let mut bytes = Vec::new();
  fs::open(directory, c"generation.hgr", expected.len() as u64)?
    .take(expected.len() as u64 + 1)
    .read_to_end(&mut bytes)?;
  ensure!(
    bytes == expected,
    "Restore generation checkpoint binding changed"
  );
  Ok(())
}

impl Stage {
  /// Package binding/layout were independently admitted by its caller. Decode all components
  /// against fresh authorized hardware before writing any restore files.
  pub fn prepare(
    parent: &File,
    package: &Package,
    hardware: &[Option<pci::Device>; 7],
    generation: [u8; 16],
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    let receipt = receipt(package, generation)?;
    let saved = checkpoint::load(package, hardware, &mut authorize)?;
    fs::private(parent)?;
    let mut random = [0; 16];
    unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
    let name = fs::name(&format!(".restore{}", name(random).to_str()?));
    let parent = parent.try_clone()?;
    let directory = fs::mkdir(&parent, &name)?;
    let mut directories = std::array::from_fn(|_| None);
    let mut files = std::array::from_fn(|_| None);
    for (slot, state) in saved.primary.devices.transports.iter().enumerate() {
      if let Some(state) = state
        .as_ref()
        .filter(|state| pci::snapshot::records::saved_kind(state) == 1)
      {
        authorize()?;
        let child = fs::mkdir(&directory, &fs::name(&format!("storage{slot}")))?;
        files[slot] = Some(pci::snapshot::prepare_disk(state, &child, &mut authorize)?);
        directories[slot] = Some(child);
      }
    }
    let store = variables::from_snapshot_at(
      &directory,
      &saved.primary.memory.nvram,
      &saved.primary.devices.flash,
      &mut authorize,
    )?;
    let variables = variables::directory(&store)?;
    let mut file = fs::create(&directory, c"generation.hgr")?;
    file.write_all(&receipt)?;
    fs::seal(&file)?;
    let mut metadata = Vec::new();
    for file in &files {
      metadata.push(
        file
          .as_ref()
          .map(|disk| disk.file().metadata())
          .transpose()?,
      );
    }
    let metadata: [Option<std::fs::Metadata>; 7] = metadata
      .try_into()
      .map_err(|_| anyhow::anyhow!("Generation disk topology changed"))?;
    ownership::Ownership::create(&directory, &receipt, &variables, &directories, &metadata)?;
    directory.sync_all()?;
    parent.sync_all()?;
    authorize()?;
    Ok(Self {
      parent,
      name,
      generation,
      ready: Generation {
        receipt,
        directory,
        directories,
        variables,
        checkpoint: saved,
        store,
        disks: Disks {
          files,
          authorize: Box::new(|| Ok(())),
        },
      },
    })
  }

  /// Caller acquires the original saved-policy guard while retaining operation/runtime ownership.
  /// Publication is exclusive; a later failure can leave a complete generation and never replaces it.
  pub fn publish<G>(
    mut self,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
    acquire: impl FnOnce() -> anyhow::Result<G>,
  ) -> anyhow::Result<Generation> {
    let _guard = acquire()?;
    authorize()?;
    fs::private(&self.parent)?;
    fs::same(&self.parent, &self.name, &self.ready.directory)?;
    self.ready.check(&mut authorize)?;
    authorize()?;
    self.ready.namespace()?;
    fs::same(&self.parent, &self.name, &self.ready.directory)?;
    let destination = name(self.generation);
    ensure!(
      unsafe {
        libc::renameatx_np(
          self.parent.as_raw_fd(),
          self.name.as_ptr(),
          self.parent.as_raw_fd(),
          destination.as_ptr(),
          libc::RENAME_EXCL,
        )
      } == 0,
      "Publish restore generation without replacement: {}",
      std::io::Error::last_os_error()
    );
    fs::same(&self.parent, &destination, &self.ready.directory)?;
    self.parent.sync_all()?;
    authorize()?;
    self.ready.namespace()?;
    fs::same(&self.parent, &destination, &self.ready.directory)?;
    Ok(self.ready)
  }
}

impl Generation {
  /// Reopen only an unchanged generation against the independently admitted source package.
  /// Any attempt marker, including malformed or partial state, rejects replay without deletion.
  pub fn open(
    parent: &File,
    package: &Package,
    hardware: &[Option<pci::Device>; 7],
    generation: [u8; 16],
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    let receipt = receipt(package, generation)?;
    fs::private(parent)?;
    let directory = fs::directory(parent, &name(generation))?;
    check_receipt(&directory, &receipt)?;
    attempt::unused(&directory)?;
    ownership::Ownership::open(&directory, &receipt)?;
    let saved = checkpoint::load(package, hardware, &mut authorize)?;
    let mut directories = std::array::from_fn(|_| None);
    let mut files = std::array::from_fn(|_| None);
    for (slot, state) in saved.primary.devices.transports.iter().enumerate() {
      if let Some(state) = state
        .as_ref()
        .filter(|state| pci::snapshot::records::saved_kind(state) == 1)
      {
        authorize()?;
        let child = fs::directory(&directory, &fs::name(&format!("storage{slot}")))?;
        files[slot] = Some(pci::snapshot::reopen_prepared_disk(
          state,
          &child,
          &mut authorize,
        )?);
        directories[slot] = Some(child);
      }
    }
    authorize()?;
    let store = variables::open_at(&directory)?;
    let variables = variables::directory(&store)?;
    let mut ready = Self {
      receipt: receipt.clone(),
      directory,
      directories,
      variables,
      checkpoint: saved,
      store,
      disks: Disks {
        files,
        authorize: Box::new(|| Ok(())),
      },
    };
    ready.check(&mut authorize)?;
    fs::same(parent, &name(generation), &ready.directory)?;
    check_receipt(&ready.directory, &receipt)?;
    authorize()?;
    ready.namespace()?;
    fs::same(parent, &name(generation), &ready.directory)?;
    Ok(ready)
  }

  fn check(&mut self, authorize: &mut impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<()> {
    authorize()?;
    self.namespace()?;
    variables::matches(&self.store, &self.checkpoint.primary.memory.nvram)?;
    for (slot, child) in self.directories.iter().enumerate() {
      if let Some(child) = child {
        fs::same(&self.directory, &fs::name(&format!("storage{slot}")), child)?;
        let state = self.checkpoint.primary.devices.transports[slot]
          .as_ref()
          .context("Missing captured disk")?;
        let disk = self.disks.files[slot]
          .as_ref()
          .context("Missing prepared disk")?;
        pci::snapshot::check_prepared(state, disk, &mut *authorize)?;
      }
    }
    authorize()?;
    self.namespace()
  }

  fn namespace(&self) -> anyhow::Result<()> {
    attempt::unused(&self.directory)?;
    self.components()
  }

  fn components(&self) -> anyhow::Result<()> {
    fs::private(&self.directory)?;
    check_receipt(&self.directory, &self.receipt)?;
    fs::same(&self.directory, c"variables", &self.variables)?;
    for (slot, child) in self.directories.iter().enumerate() {
      if let Some(child) = child {
        fs::same(&self.directory, &fs::name(&format!("storage{slot}")), child)?;
      }
    }
    let mut metadata = Vec::new();
    for file in &self.disks.files {
      metadata.push(
        file
          .as_ref()
          .map(|disk| disk.file().metadata())
          .transpose()?,
      );
    }
    let metadata: [Option<std::fs::Metadata>; 7] = metadata
      .try_into()
      .map_err(|_| anyhow::anyhow!("Generation disk topology changed"))?;
    ownership::Ownership::open(&self.directory, &self.receipt)?.check(
      &self.directory,
      &self.receipt,
      &self.variables,
      &self.directories,
      &metadata,
    )?;
    Ok(())
  }
}
