use anyhow::{ensure, Context};
use model::guest::files::{Uploaded, COMMIT, UPLOAD_LIMIT};
use sha2::{Digest, Sha256};
use std::{
  io::{Read, Write},
  path::Path,
};

pub fn receive(
  path: &Path,
  size: u64,
  sha256: &str,
  input: &mut impl Read,
) -> anyhow::Result<Uploaded> {
  ensure!(
    size <= UPLOAD_LIMIT
      && sha256.len() == 64
      && sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
    "Invalid guest upload size or checksum"
  );
  let mut writer = super::writer::prepare(path, size)?;
  let mut hash = Sha256::new();
  let mut buffer = vec![0; super::CHUNK];
  let mut remaining = size;
  while remaining != 0 {
    let length = remaining.min(buffer.len() as u64) as usize;
    input
      .read_exact(&mut buffer[..length])
      .context("Guest upload ended before all bytes arrived")?;
    hash.update(&buffer[..length]);
    writer.append(&buffer[..length])?;
    remaining -= length as u64;
  }
  let mut marker = [0; COMMIT.len()];
  input
    .read_exact(&mut marker)
    .context("Guest upload was not committed")?;
  ensure!(marker == *COMMIT, "Invalid guest upload completion marker");
  ensure!(
    input.read(&mut buffer[..1])? == 0,
    "Guest upload has trailing bytes"
  );
  let actual = format!("{:x}", hash.finalize());
  ensure!(
    actual == sha256.to_ascii_lowercase(),
    "Guest upload checksum mismatch"
  );
  Ok(Uploaded {
    file: writer.publish()?,
    sha256: actual,
  })
}

pub fn command(args: &[std::ffi::OsString]) -> anyhow::Result<()> {
  ensure!(args.len() == 3, "Invalid guest upload arguments");
  let size = args[2]
    .to_str()
    .context("Invalid guest upload size")?
    .parse::<u64>()?;
  ensure!(size <= UPLOAD_LIMIT, "Guest upload size exceeds bounds");
  let mut kind: libc::c_int = 0;
  let mut width = std::mem::size_of_val(&kind) as libc::socklen_t;
  ensure!(
    unsafe {
      libc::getsockopt(
        libc::STDIN_FILENO,
        libc::SOL_SOCKET,
        libc::SO_DOMAIN,
        (&mut kind as *mut libc::c_int).cast(),
        &mut width,
      )
    } == 0
      && kind == libc::AF_UNIX,
    "Guest upload requires a private local stream"
  );
  width = std::mem::size_of_val(&kind) as libc::socklen_t;
  ensure!(
    unsafe {
      libc::getsockopt(
        libc::STDIN_FILENO,
        libc::SOL_SOCKET,
        libc::SO_TYPE,
        (&mut kind as *mut libc::c_int).cast(),
        &mut width,
      )
    } == 0
      && kind == libc::SOCK_STREAM,
    "Guest upload requires a private local stream"
  );
  let mut input = std::io::stdin().lock();
  let mut expected = [0; 32];
  input
    .read_exact(&mut expected)
    .context("Guest upload checksum header is incomplete")?;
  let sha256 = expected
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
  let receipt = receive(Path::new(&args[1]), size, &sha256, &mut input)?;
  writeln!(
    std::io::stdout().lock(),
    "{}",
    serde_json::to_string(&receipt)?
  )?;
  Ok(())
}
