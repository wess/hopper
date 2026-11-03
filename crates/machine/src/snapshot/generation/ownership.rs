use super::fs;
use anyhow::ensure;
use sha2::{Digest, Sha256};
use std::{
  fs::{File, Metadata},
  io::{Read, Write},
  os::unix::fs::MetadataExt,
};
const SIZE: usize = 387;

pub(super) struct Ownership(Vec<u8>);
fn identity(metadata: &Metadata, bytes: &mut Vec<u8>) {
  bytes.extend_from_slice(&metadata.dev().to_le_bytes());
  bytes.extend_from_slice(&metadata.ino().to_le_bytes());
}
fn encode(
  directory: &File,
  receipt: &[u8],
  variables: &File,
  directories: &[Option<File>; 7],
  disks: &[Option<Metadata>; 7],
) -> anyhow::Result<Vec<u8>> {
  let mut bytes = Vec::from(b"HGO1");
  bytes.extend_from_slice(&Sha256::digest(receipt));
  identity(&directory.metadata()?, &mut bytes);
  identity(&variables.metadata()?, &mut bytes);
  for (child, disk) in directories.iter().zip(disks) {
    match (child, disk) {
      (Some(child), Some(disk)) => {
        bytes.push(1);
        identity(&child.metadata()?, &mut bytes);
        identity(disk, &mut bytes);
        bytes.extend_from_slice(&disk.len().to_le_bytes());
      }
      (None, None) => bytes.extend_from_slice(&[0; 41]),
      _ => anyhow::bail!("Generation ownership topology differs"),
    }
  }
  bytes.extend_from_slice(&Sha256::digest(&bytes));
  ensure!(
    bytes.len() == SIZE,
    "Generation ownership receipt exceeds its bound"
  );
  Ok(bytes)
}

impl Ownership {
  pub fn create(
    directory: &File,
    receipt: &[u8],
    variables: &File,
    directories: &[Option<File>; 7],
    disks: &[Option<Metadata>; 7],
  ) -> anyhow::Result<()> {
    let bytes = encode(directory, receipt, variables, directories, disks)?;
    let mut file = fs::create(directory, c"ownership.hgo")?;
    file.write_all(&bytes)?;
    fs::seal(&file)
  }

  pub fn open(directory: &File, receipt: &[u8]) -> anyhow::Result<Self> {
    let mut bytes = Vec::new();
    fs::open(directory, c"ownership.hgo", SIZE as u64)?
      .take(SIZE as u64 + 1)
      .read_to_end(&mut bytes)?;
    ensure!(
      bytes.len() == SIZE
        && &bytes[..4] == b"HGO1"
        && bytes[4..36] == Sha256::digest(receipt)[..]
        && bytes[SIZE - 32..] == Sha256::digest(&bytes[..SIZE - 32])[..],
      "Invalid generation ownership receipt; recovery is required"
    );
    let mut current = Vec::new();
    identity(&directory.metadata()?, &mut current);
    ensure!(
      bytes[36..52] == current,
      "Generation directory identity changed"
    );
    Ok(Self(bytes))
  }

  pub fn check(
    &self,
    directory: &File,
    receipt: &[u8],
    variables: &File,
    directories: &[Option<File>; 7],
    disks: &[Option<Metadata>; 7],
  ) -> anyhow::Result<()> {
    ensure!(
      Self::open(directory, receipt)?.0 == self.0
        && encode(directory, receipt, variables, directories, disks)? == self.0,
      "Generation resource identity or capacity changed; recovery is required"
    );
    Ok(())
  }
}
