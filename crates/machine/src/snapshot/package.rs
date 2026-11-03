//! pinned private snapshot stages, checksummed writes and exclusive atomic publication.
//! abandoned stages are preserved; recovery/garbage collection must establish ownership first.

use super::manifest::{Binding, Checked, Entry, Layout, Manifest, Record, MAX};
use anyhow::ensure;
use std::{
  collections::BTreeMap,
  ffi::CString,
  fs::File,
  io::{BufReader, Read, Write},
};
mod description;
pub(crate) mod fs;
pub use description::Description;
mod inventory;
pub use inventory::{inventory, Summary};
mod sealed;
mod verify;
mod writer;
pub use sealed::{Receipt, Sealed};

pub struct Stage {
  parent: File,
  directory: File,
  name: CString,
  nonce: [u8; 16],
  binding: Binding,
  layout: Layout,
  entries: BTreeMap<Record, Entry>,
  failed: bool,
}

pub struct Package {
  directory: File,
  manifest: Manifest,
  layout: Layout,
}

fn identity(bytes: [u8; 16]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Stage {
  /// The caller retains VM operation ownership and the original authorization throughout.
  pub fn new(
    parent: &File,
    binding: Binding,
    layout: Layout,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    fs::private(parent)?;
    let entries = layout
      .records()?
      .into_iter()
      .map(|record| Entry {
        record,
        length: 36,
        checksum: [0; 32],
      })
      .collect();
    Manifest::new(binding, layout, entries)?;
    let mut random = [0; 16];
    unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
    let name = fs::name(&format!(".stage{}", identity(random)));
    let parent = parent.try_clone()?;
    let directory = fs::mkdir(&parent, &name)?;
    authorize()?;
    Ok(Self {
      parent,
      directory,
      name,
      nonce: random,
      binding,
      layout,
      entries: BTreeMap::new(),
      failed: false,
    })
  }

  /// A failed encoder poisons the whole stage, including errors after its final write.
  pub fn write(
    &mut self,
    record: Record,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
    encode: impl FnOnce(&mut dyn Write) -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    authorize()?;
    ensure!(
      !self.failed && !self.entries.contains_key(&record),
      "Snapshot stage is failed or duplicated"
    );
    fs::private(&self.directory)?;
    let limit = self.layout.limit(record)?;
    self.failed = true;
    let file = fs::create(&self.directory, &fs::name(&record.name()))?;
    let mut output = writer::Writer::new(file, &mut authorize, limit);
    encode(&mut output)?;
    let entry = output.finish(record)?;
    self.entries.insert(record, entry);
    self.failed = false;
    Ok(())
  }

  /// Only trusted image encoders receive a directory capability, never an agent path.
  pub fn image(
    &mut self,
    slot: u8,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
    encode: impl FnOnce(&File, &mut dyn FnMut() -> anyhow::Result<()>) -> anyhow::Result<Vec<u8>>,
  ) -> anyhow::Result<()> {
    authorize()?;
    ensure!(
      !self.failed && matches!(self.layout.devices.get(slot as usize), Some(1 | 4)),
      "Snapshot image slot is unsupported or stage failed"
    );
    self.failed = true;
    let directory = fs::mkdir(&self.directory, &fs::name(&format!("storage{slot}")))?;
    let bytes = encode(&directory, &mut authorize)?;
    directory.sync_all()?;
    authorize()?;
    self.failed = false;
    self.write(Record::Backend(slot), authorize, |output| {
      Ok(output.write_all(&bytes)?)
    })
  }

  /// Prepare a complete private stage for a host-controlled commit, without publishing it.
  /// A receipt is not authorization; the host retains its original operation/runtime owners.
  pub fn seal(self, mut authorize: impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<Sealed> {
    let sealed = self.seal_inner(&mut authorize)?;
    sealed.sync()?;
    authorize()?;
    Ok(sealed)
  }

  /// Errors after rename may leave a complete package. Never retry by replacing it.
  /// `acquire` excludes policy changes through rename, sync and the final authorization.
  pub fn publish<G>(
    self,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
    acquire: impl FnOnce() -> anyhow::Result<G>,
  ) -> anyhow::Result<Package> {
    self.seal_inner(&mut authorize)?.publish(authorize, acquire)
  }

  fn seal_inner(self, mut authorize: impl FnMut() -> anyhow::Result<()>) -> anyhow::Result<Sealed> {
    authorize()?;
    ensure!(!self.failed, "Snapshot stage failed");
    fs::private(&self.parent)?;
    fs::same(&self.parent, &self.name, &self.directory)?;
    let manifest = Manifest::new(
      self.binding,
      self.layout,
      self.entries.into_values().collect(),
    )?;
    let mut file = fs::create(&self.directory, c"manifest.hsm")?;
    file.write_all(&manifest.encode())?;
    fs::seal(&file)?;
    drop(file);
    self.directory.sync_all()?;
    Sealed::new(
      self.parent,
      self.directory,
      self.name,
      self.nonce,
      self.binding.snapshot,
      manifest,
      self.layout,
    )
  }
}

impl Package {
  pub fn binding(&self) -> Binding {
    self.manifest.binding()
  }

  /// Inspect one pinned package. Its claimed configuration still needs independent validation.
  pub fn inspect(
    parent: &File,
    machine: [u8; 16],
    snapshot: [u8; 16],
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    fs::private(parent)?;
    let directory = fs::directory(parent, &fs::name(&identity(snapshot)))?;
    let mut bytes = Vec::new();
    fs::open(&directory, c"manifest.hsm", MAX as u64)?
      .take(MAX as u64 + 1)
      .read_to_end(&mut bytes)?;
    let manifest = Manifest::inspect(&bytes)?;
    ensure!(
      manifest.binding().machine == machine && manifest.binding().snapshot == snapshot,
      "Snapshot inspection identity mismatch"
    );
    fs::same(parent, &fs::name(&identity(snapshot)), &directory)?;
    authorize()?;
    Ok(Self {
      directory,
      layout: manifest.layout(),
      manifest,
    })
  }

  pub fn manifest_fingerprint(&self) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(self.manifest.encode()).into()
  }

  pub(crate) fn manifest_bytes(&self) -> Vec<u8> {
    self.manifest.encode()
  }

  pub fn layout(&self) -> Layout {
    self.layout
  }

  pub fn open(
    parent: &File,
    binding: Binding,
    layout: Layout,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    fs::private(parent)?;
    let directory = fs::directory(parent, &fs::name(&identity(binding.snapshot)))?;
    let mut bytes = Vec::new();
    fs::open(&directory, c"manifest.hsm", MAX as u64)?
      .take(MAX as u64 + 1)
      .read_to_end(&mut bytes)?;
    let manifest = Manifest::decode(&bytes, binding, layout)?;
    authorize()?;
    Ok(Self {
      directory,
      manifest,
      layout,
    })
  }

  /// Decode the component and call finish before using any restored state.
  pub fn reader<A: FnMut() -> anyhow::Result<()>>(
    &self,
    record: Record,
    mut authorize: A,
  ) -> anyhow::Result<Checked<'_, BufReader<File>, A>> {
    authorize()?;
    fs::private(&self.directory)?;
    let file = fs::open(
      &self.directory,
      &fs::name(&record.name()),
      self.layout.limit(record)?,
    )?;
    authorize()?;
    self
      .manifest
      .reader(record, BufReader::with_capacity(65536, file), authorize)
  }

  pub fn image(
    &self,
    slot: u8,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<File> {
    authorize()?;
    ensure!(
      matches!(self.layout.devices.get(slot as usize), Some(1 | 4)),
      "Snapshot image slot is unsupported"
    );
    let directory = fs::directory(&self.directory, &fs::name(&format!("storage{slot}")))?;
    authorize()?;
    Ok(directory)
  }
}
