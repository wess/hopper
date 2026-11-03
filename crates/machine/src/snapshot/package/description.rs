use super::{fs, Manifest, Sealed};
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  fs::File,
  io::{Read, Write},
  os::fd::AsRawFd,
};

const MAX: usize = 209;

pub struct Description {
  name: String,
  created: u64,
}

impl Description {
  pub fn new(name: &str, created: u64) -> anyhow::Result<Self> {
    let name = name.trim();
    ensure!(
      !name.is_empty() && name.len() <= 100 && !name.chars().any(char::is_control),
      "Choose a snapshot name of 1–100 bytes without control characters"
    );
    ensure!(
      (1..=253402300799).contains(&created),
      "Snapshot creation time is invalid"
    );
    Ok(Self {
      name: name.into(),
      created,
    })
  }

  pub fn name(&self) -> &str {
    &self.name
  }
  pub fn created(&self) -> u64 {
    self.created
  }

  pub(super) fn checksum(&self, manifest: &Manifest) -> [u8; 32] {
    Sha256::digest(self.encode(manifest)).into()
  }

  fn encode(&self, manifest: &Manifest) -> Vec<u8> {
    let mut bytes = Vec::from(b"HSN1");
    bytes.extend_from_slice(&manifest.binding().machine);
    bytes.extend_from_slice(&manifest.binding().snapshot);
    bytes.extend_from_slice(&Sha256::digest(manifest.encode()));
    bytes.extend_from_slice(&self.created.to_le_bytes());
    bytes.push(self.name.len() as u8);
    bytes.extend_from_slice(self.name.as_bytes());
    bytes.extend_from_slice(&Sha256::digest(&bytes));
    bytes
  }
}

impl Sealed {
  /// Host-authored metadata joins the same exclusive package rename, never a later index write.
  pub fn describe(
    &mut self,
    description: &Description,
    mut authorize: impl FnMut() -> anyhow::Result<()>,
  ) -> anyhow::Result<()> {
    authorize()?;
    fs::same(&self.parent, &self.name, &self.package.directory)?;
    let bytes = description.encode(&self.package.manifest);
    let mut file = fs::create(&self.package.directory, c"description.hsn")?;
    file.write_all(&bytes)?;
    fs::seal(&file)?;
    self.package.directory.sync_all()?;
    self.description = Some(Sha256::digest(&bytes).into());
    authorize()?;
    Ok(())
  }
}

pub(super) fn read(directory: &File, manifest: &Manifest) -> anyhow::Result<Option<Description>> {
  let mut info = std::mem::MaybeUninit::<libc::stat>::uninit();
  let result = unsafe {
    libc::fstatat(
      directory.as_raw_fd(),
      c"description.hsn".as_ptr(),
      info.as_mut_ptr(),
      libc::AT_SYMLINK_NOFOLLOW,
    )
  };
  if result != 0 {
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOENT) {
      return Ok(None);
    }
    return Err(error.into());
  }
  let mut bytes = Vec::new();
  fs::open(directory, c"description.hsn", MAX as u64)?
    .take(MAX as u64 + 1)
    .read_to_end(&mut bytes)?;
  ensure!(
    (110..=MAX).contains(&bytes.len()) && &bytes[..4] == b"HSN1",
    "Invalid snapshot description"
  );
  let end = bytes.len() - 32;
  ensure!(
    Sha256::digest(&bytes[..end])[..] == bytes[end..],
    "Snapshot description checksum mismatch"
  );
  ensure!(
    bytes[4..20] == manifest.binding().machine
      && bytes[20..36] == manifest.binding().snapshot
      && bytes[36..68] == Sha256::digest(manifest.encode())[..],
    "Snapshot description identity mismatch"
  );
  ensure!(
    77 + bytes[76] as usize == end,
    "Snapshot description length mismatch"
  );
  let name = std::str::from_utf8(&bytes[77..end])?;
  let description = Description::new(name, u64::from_le_bytes(bytes[68..76].try_into()?))?;
  ensure!(
    description.name == name,
    "Snapshot description is not canonical"
  );
  Ok(Some(description))
}
