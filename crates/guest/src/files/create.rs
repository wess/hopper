use anyhow::{ensure, Context};
use std::{
  fs::File,
  io::{Read, Write},
  os::fd::AsRawFd,
  path::Path,
};

pub fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<model::guest::files::Receipt> {
  ensure!(
    bytes.len() <= model::guest::files::CREATE_LIMIT,
    "Guest file payload exceeds bounds"
  );
  let mut writer = super::writer::prepare(path, bytes.len() as u64)?;
  writer.append(bytes)?;
  writer.publish()
}

pub fn command(args: &[std::ffi::OsString]) -> anyhow::Result<()> {
  ensure!(args.len() == 3, "Invalid guest file creation arguments");
  let length = args[2]
    .to_str()
    .context("Invalid guest file payload length")?
    .parse::<usize>()?;
  ensure!(
    length <= model::guest::files::CREATE_LIMIT,
    "Guest file payload exceeds bounds"
  );
  let input = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_DUPFD_CLOEXEC, 3) };
  ensure!(input >= 0, "Open guest file payload descriptor");
  use std::os::fd::FromRawFd;
  let mut input = unsafe { File::from_raw_fd(input) };
  let metadata = input.metadata()?;
  let seals = unsafe { libc::fcntl(input.as_raw_fd(), libc::F_GET_SEALS) };
  let required = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
  ensure!(
    metadata.is_file()
      && metadata.len() == length as u64
      && seals >= 0
      && seals & required == required,
    "Guest file payload must be bounded and sealed"
  );
  let mut bytes = vec![0; length];
  input.read_exact(&mut bytes)?;
  let receipt = write(Path::new(&args[1]), &bytes)?;
  writeln!(
    std::io::stdout().lock(),
    "{}",
    serde_json::to_string(&receipt)?
  )?;
  Ok(())
}
