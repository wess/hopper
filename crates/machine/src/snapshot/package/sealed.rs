use super::{fs, identity, Binding, Layout, Manifest, Package, MAX};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  ffi::CString,
  fs::File,
  io::Read,
  os::{fd::AsRawFd, unix::fs::MetadataExt},
};

pub use model::native::snapshot::Receipt;

pub struct Sealed {
  pub(super) parent: File,
  pub(super) name: CString,
  snapshot: [u8; 16],
  pub(super) package: Package,
  receipt: Receipt,
  pub(super) description: Option<[u8; 32]>,
}

impl Sealed {
  pub(super) fn new(
    parent: File,
    directory: File,
    name: CString,
    nonce: [u8; 16],
    snapshot: [u8; 16],
    manifest: Manifest,
    layout: Layout,
  ) -> anyhow::Result<Self> {
    let metadata = directory.metadata()?;
    let receipt = Receipt {
      nonce,
      device: metadata.dev(),
      inode: metadata.ino(),
      manifest: Sha256::digest(manifest.encode()).into(),
    };
    Ok(Self {
      parent,
      name,
      snapshot,
      package: Package {
        directory,
        manifest,
        layout,
      },
      receipt,
      description: None,
    })
  }

  pub(super) fn sync(&self) -> anyhow::Result<()> {
    self.parent.sync_all()?;
    Ok(())
  }

  pub fn receipt(&self) -> Receipt {
    self.receipt
  }

  /// Reopen only inside the already-authorized pinned namespace, using expected identity/layout.
  /// Component schema and image receipts still require validation before hardware restore.
  pub fn open(
    parent: &File,
    receipt: Receipt,
    binding: Binding,
    layout: Layout,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<Self> {
    authorize()?;
    fs::private(parent)?;
    let name = fs::name(&format!(".stage{}", identity(receipt.nonce)));
    let directory = fs::directory(parent, &name)?;
    let metadata = directory.metadata()?;
    ensure!(
      (metadata.dev(), metadata.ino()) == (receipt.device, receipt.inode),
      "Snapshot stage does not match its receipt"
    );
    let mut bytes = Vec::new();
    fs::open(&directory, c"manifest.hsm", MAX as u64)?
      .take(MAX as u64 + 1)
      .read_to_end(&mut bytes)?;
    ensure!(
      <[u8; 32]>::from(Sha256::digest(&bytes)) == receipt.manifest,
      "Snapshot manifest does not match its receipt"
    );
    let manifest = Manifest::decode(&bytes, binding, layout)?;
    authorize()?;
    Ok(Self {
      parent: parent.try_clone()?,
      name,
      snapshot: binding.snapshot,
      package: Package {
        directory,
        manifest,
        layout,
      },
      receipt,
      description: None,
    })
  }

  /// Only the host holding original policy/operation/runtime grants may acquire the commit guard.
  /// Cancellation after rename can leave a complete package; never replace or delete it blindly.
  pub fn publish<G>(
    self,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
    acquire: impl FnOnce() -> anyhow::Result<G>,
  ) -> anyhow::Result<Package> {
    let _commit = acquire()?;
    authorize()?;
    fs::private(&self.parent)?;
    fs::same(&self.parent, &self.name, &self.package.directory)?;
    let mut bytes = Vec::new();
    fs::open(&self.package.directory, c"manifest.hsm", MAX as u64)?
      .take(MAX as u64 + 1)
      .read_to_end(&mut bytes)?;
    ensure!(
      <[u8; 32]>::from(Sha256::digest(&bytes)) == self.receipt.manifest,
      "Snapshot manifest changed before commit"
    );
    let description = super::description::read(&self.package.directory, &self.package.manifest)?;
    if let Some(expected) = self.description {
      let description = description
        .ok_or_else(|| anyhow::anyhow!("Snapshot description disappeared before commit"))?;
      ensure!(
        description.checksum(&self.package.manifest) == expected,
        "Snapshot description changed before commit"
      );
    }
    let destination = fs::name(&identity(self.snapshot));
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
      "Publish snapshot without replacement: {}",
      std::io::Error::last_os_error()
    );
    fs::same(&self.parent, &destination, &self.package.directory)?;
    self.parent.sync_all()?;
    authorize()?;
    Ok(self.package)
  }
}
