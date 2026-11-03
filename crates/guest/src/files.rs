use anyhow::ensure;
use std::{
  fs::{File, Metadata, OpenOptions},
  os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt},
  path::Path,
};

pub use model::guest::files::{Stamp, CHUNK, LIMIT};
pub mod browse;
#[cfg(target_os = "linux")]
pub mod create;
#[cfg(target_os = "linux")]
pub mod upload;
#[cfg(target_os = "linux")]
mod writer;

pub struct Reader {
  file: File,
  stamp: Stamp,
}

pub struct Chunk {
  pub offset: u64,
  pub bytes: Vec<u8>,
  pub eof: bool,
}

pub(super) fn measure(metadata: Metadata) -> anyhow::Result<Stamp> {
  ensure!(
    metadata.is_file() && metadata.len() <= LIMIT,
    "Guest transfer requires a bounded regular file"
  );
  Ok(Stamp {
    device: metadata.dev(),
    inode: metadata.ino(),
    size: metadata.len(),
    modified: (metadata.mtime(), metadata.mtime_nsec()),
    changed: (metadata.ctime(), metadata.ctime_nsec()),
  })
}

pub fn open(path: &Path) -> anyhow::Result<Reader> {
  use std::os::unix::ffi::OsStrExt;
  let name = path.as_os_str().as_bytes();
  ensure!(
    path.is_absolute() && (1..=4096).contains(&name.len()) && !name.contains(&0),
    "Guest file path must be bounded and absolute"
  );
  // called only in a guest worker after its execution identity has been applied.
  let flags = libc::O_NOFOLLOW | libc::O_CLOEXEC;
  #[cfg(target_os = "linux")]
  let flags = flags | libc::O_PATH;
  #[cfg(not(target_os = "linux"))]
  let flags = flags | libc::O_NONBLOCK;
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(flags)
    .open(path)?;
  let stamp = measure(file.metadata()?)?;
  #[cfg(target_os = "linux")]
  let file = {
    use std::os::fd::AsRawFd;
    let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
      unsafe { libc::fstatfs(file.as_raw_fd(), &mut filesystem) } == 0,
      "Inspect guest file filesystem"
    );
    ensure!(
      ![
        i128::from(libc::PROC_SUPER_MAGIC),
        i128::from(libc::SYSFS_MAGIC)
      ]
      .contains(&i128::from(filesystem.f_type)),
      "Guest transfer cannot read process or kernel pseudo-files"
    );
    // validate with O_PATH before activating any device's open operation.
    let opened = OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
      .open(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
    ensure!(
      measure(opened.metadata()?)? == stamp,
      "Guest file changed while opening"
    );
    opened
  };
  Ok(Reader { file, stamp })
}

impl Reader {
  pub fn stamp(&self) -> Stamp {
    self.stamp
  }

  pub fn read(&self, offset: u64, length: usize) -> anyhow::Result<Chunk> {
    ensure!(
      (1..=CHUNK).contains(&length) && offset <= self.stamp.size,
      "Invalid guest file range"
    );
    ensure!(
      measure(self.file.metadata()?)? == self.stamp,
      "Guest file changed during transfer"
    );
    let count = (self.stamp.size - offset).min(length as u64) as usize;
    let mut bytes = vec![0; count];
    self.file.read_exact_at(&mut bytes, offset)?;
    ensure!(
      measure(self.file.metadata()?)? == self.stamp,
      "Guest file changed during transfer"
    );
    Ok(Chunk {
      offset,
      eof: offset + count as u64 == self.stamp.size,
      bytes,
    })
  }
}

#[cfg(target_os = "linux")]
pub fn command(args: &[std::ffi::OsString]) -> anyhow::Result<()> {
  use std::io::Write;
  ensure!(
    matches!(args.len(), 4 | 5),
    "Invalid guest file read arguments"
  );
  let text = |index: usize| {
    args[index]
      .to_str()
      .ok_or_else(|| anyhow::anyhow!("Invalid file range"))
  };
  let offset = text(2)?.parse::<u64>()?;
  let length = text(3)?.parse::<usize>()?;
  let expected = args
    .get(4)
    .map(|value| {
      let text = value
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid guest file stamp"))?;
      ensure!(text.len() <= 512, "Guest file stamp exceeds bounds");
      serde_json::from_str::<Stamp>(text).map_err(Into::into)
    })
    .transpose()?;
  let reader = open(Path::new(&args[1]))?;
  if let Some(expected) = expected {
    ensure!(
      expected.valid() && expected == reader.stamp(),
      "Guest file stamp changed"
    );
  }
  let chunk = reader.read(offset, length)?;
  let receipt = model::guest::files::Receipt {
    stamp: reader.stamp(),
    offset,
    length: chunk.bytes.len(),
    eof: chunk.eof,
  };
  std::io::stdout().lock().write_all(&chunk.bytes)?;
  writeln!(
    std::io::stderr().lock(),
    "{}",
    serde_json::to_string(&receipt)?
  )?;
  Ok(())
}
