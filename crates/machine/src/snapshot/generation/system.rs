use super::{attempt, check_receipt, fs, name, ownership::Ownership};
use crate::{
  runtime::variables::{self, Variables},
  snapshot::{
    manifest::{Binding, Manifest, MAX},
    selection::{self, Phase, Record},
    storage::Current,
  },
};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read};

/// Current mutable resources only. No captured memory, CPU or device execution state is decoded.
pub struct System {
  ownership: Ownership,
  root: File,
  parent: File,
  directory: File,
  variables: File,
  directories: [Option<File>; 7],
  receipt: Vec<u8>,
  marker: Vec<u8>,
  selection: Record,
  layout: crate::snapshot::manifest::Layout,
  store: Variables,
  disks: [Option<Current>; 7],
}

impl System {
  /// Selection is independently authorized by the host. Recheck hardware compatibility before boot.
  /// The source snapshot is not needed for fresh boot; its provenance is retained in HGR1/HGA1.
  pub fn open(
    root: &File,
    expected: Record,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    ensure!(
      expected.phase == Phase::Active,
      "System startup requires an active generation"
    );
    ensure!(
      selection::read_at(root, expected.machine)? == expected,
      "Active selection changed"
    );
    let root = root.try_clone()?;
    let parent = fs::directory(&root, c"generations")?;
    let directory = fs::directory(&parent, &name(expected.generation))?;
    let mut receipt = Vec::new();
    fs::open(&directory, c"generation.hgr", (MAX + 52) as u64)?
      .take((MAX + 53) as u64)
      .read_to_end(&mut receipt)?;
    ensure!(
      (52..=MAX + 52).contains(&receipt.len())
        && &receipt[..4] == b"HGR1"
        && receipt[4..20] == expected.generation,
      "Invalid current generation receipt"
    );
    let end = receipt.len() - 32;
    ensure!(
      receipt[end..] == Sha256::digest(&receipt[..end])[..],
      "Current generation receipt checksum mismatch"
    );
    let manifest = Manifest::inspect(&receipt[20..end])?;
    ensure!(
      manifest.binding()
        == Binding {
          machine: expected.machine,
          snapshot: expected.snapshot,
          configuration: expected.configuration
        },
      "Current generation differs from active selection"
    );
    let layout = manifest.layout();
    let marker = attempt::executed(&directory, &receipt)?;
    let ownership = Ownership::open(&directory, &receipt)?;
    let mut directories = std::array::from_fn(|_| None);
    let mut disks = std::array::from_fn(|_| None);
    for (slot, kind) in layout.devices.iter().enumerate() {
      authorize()?;
      if *kind == 1 {
        let child = fs::directory(&directory, &fs::name(&format!("storage{slot}")))?;
        disks[slot] = Some(Current::open(&child, &mut authorize)?);
        directories[slot] = Some(child);
      }
    }
    authorize()?;
    let store = variables::open_at(&directory)?;
    let variables = variables::directory(&store)?;
    let system = Self {
      ownership,
      root,
      parent,
      directory,
      variables,
      directories,
      receipt,
      marker,
      selection: expected,
      layout,
      store,
      disks,
    };
    system.check(&mut authorize)?;
    Ok(system)
  }

  pub fn layout(&self) -> crate::snapshot::manifest::Layout {
    self.layout
  }

  pub fn check(&self, mut authorize: impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<()> {
    authorize()?;
    ensure!(
      selection::read_at(&self.root, self.selection.machine)? == self.selection,
      "Active generation selection changed"
    );
    fs::same(&self.root, c"generations", &self.parent)?;
    fs::same(
      &self.parent,
      &name(self.selection.generation),
      &self.directory,
    )?;
    check_receipt(&self.directory, &self.receipt)?;
    attempt::check(&self.directory, &self.marker)?;
    fs::same(&self.directory, c"variables", &self.variables)?;
    for (slot, child) in self.directories.iter().enumerate() {
      if let Some(child) = child {
        fs::same(&self.directory, &fs::name(&format!("storage{slot}")), child)?;
        self.disks[slot]
          .as_ref()
          .expect("owned current disk")
          .check(&mut authorize)?;
      }
    }
    let mut metadata = Vec::new();
    for file in &self.disks {
      metadata.push(
        file
          .as_ref()
          .map(|disk| disk.file().metadata())
          .transpose()?,
      );
    }
    let metadata: [Option<std::fs::Metadata>; 7] = metadata
      .try_into()
      .map_err(|_| anyhow::anyhow!("Current disk topology changed"))?;
    self.ownership.check(
      &self.directory,
      &self.receipt,
      &self.variables,
      &self.directories,
      &metadata,
    )?;
    authorize()
  }

  pub fn admit(
    self,
    authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<(Variables, [Option<Current>; 7])> {
    self.check(authorize)?;
    Ok((self.store, self.disks))
  }
}
