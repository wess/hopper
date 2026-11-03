use anyhow::{ensure, Context};
use model::guest::files::{Upload, COMMIT, CREATE_LIMIT};
use std::{
  fs::File,
  io,
  net::Shutdown,
  os::fd::{AsRawFd, OwnedFd},
  os::unix::net::UnixStream,
};

pub(super) struct Producer {
  stream: Option<UnixStream>,
  pub size: u64,
  pub offset: u64,
  marker: usize,
  pub committed: bool,
}

pub(super) fn prepare(spec: &Upload) -> anyhow::Result<(Producer, File)> {
  ensure!(spec.command().is_some(), "Invalid guest upload");
  let (sender, receiver) = UnixStream::pair()?;
  let capacity: libc::c_int = 64 << 10;
  ensure!(
    unsafe {
      libc::setsockopt(
        sender.as_raw_fd(),
        libc::SOL_SOCKET,
        libc::SO_SNDBUF,
        (&capacity as *const libc::c_int).cast(),
        std::mem::size_of_val(&capacity) as libc::socklen_t,
      )
    } == 0,
    "Bound guest upload buffer: {}",
    io::Error::last_os_error()
  );
  let mut digest = [0; 32];
  for (index, byte) in digest.iter_mut().enumerate() {
    *byte = u8::from_str_radix(&spec.sha256[index * 2..index * 2 + 2], 16)?;
  }
  ensure!(
    send(&sender, &digest)? == digest.len(),
    "Guest upload header was incomplete"
  );
  Ok((
    Producer {
      stream: Some(sender),
      size: spec.size,
      offset: 0,
      marker: 0,
      committed: false,
    },
    File::from(OwnedFd::from(receiver)),
  ))
}

fn send(stream: &UnixStream, bytes: &[u8]) -> anyhow::Result<usize> {
  loop {
    let count = unsafe {
      libc::send(
        stream.as_raw_fd(),
        bytes.as_ptr().cast(),
        bytes.len(),
        libc::MSG_DONTWAIT | libc::MSG_NOSIGNAL,
      )
    };
    if count >= 0 {
      return Ok(count as usize);
    }
    let error = io::Error::last_os_error();
    match error.kind() {
      io::ErrorKind::Interrupted => continue,
      io::ErrorKind::WouldBlock => return Ok(0),
      _ => return Err(error.into()),
    }
  }
}

impl Producer {
  pub fn append(&mut self, offset: u64, bytes: &[u8]) -> anyhow::Result<()> {
    ensure!(
      self.marker == 0 && !self.committed,
      "Guest upload is committing"
    );
    ensure!(
      offset == self.offset
        && !bytes.is_empty()
        && bytes.len() <= CREATE_LIMIT
        && bytes.len() as u64 <= self.size - self.offset,
      "Invalid guest upload chunk or offset"
    );
    let stream = self
      .stream
      .as_ref()
      .context("Guest upload channel is closed")?;
    self.offset += send(stream, bytes)? as u64;
    Ok(())
  }

  pub fn commit(&mut self) -> anyhow::Result<()> {
    if self.committed {
      return Ok(());
    }
    ensure!(self.offset == self.size, "Guest upload is incomplete");
    let stream = self
      .stream
      .as_ref()
      .context("Guest upload channel is closed")?;
    self.marker += send(stream, &COMMIT[self.marker..])?;
    if self.marker == COMMIT.len() {
      stream.shutdown(Shutdown::Write)?;
      self.committed = true;
      self.close();
    }
    Ok(())
  }

  pub fn close(&mut self) {
    self.stream.take();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use model::guest::Role;
  use std::io::Read;

  fn spec(size: u64) -> Upload {
    Upload {
      path: "/home/hopper/new".into(),
      size,
      sha256: "12".repeat(32),
      timeout_seconds: 30,
      role: Role::Normal,
    }
  }

  #[test]
  fn header_chunks_commit_and_close_are_ordered() {
    let (mut producer, mut input) = prepare(&spec(3)).unwrap();
    assert!(producer.commit().is_err());
    producer.append(0, b"abc").unwrap();
    assert_eq!(producer.offset, 3);
    assert!(producer.append(0, b"x").is_err());
    producer.commit().unwrap();
    producer.commit().unwrap();
    assert!(producer.committed);
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).unwrap();
    assert_eq!(&bytes[..32], &[0x12; 32]);
    assert_eq!(&bytes[32..35], b"abc");
    assert_eq!(&bytes[35..], COMMIT);
  }

  #[test]
  fn stalled_receiver_bounds_buffer_and_abort_omits_commit() {
    let (mut producer, mut input) = prepare(&spec(1 << 30)).unwrap();
    let chunk = vec![7; CREATE_LIMIT];
    for _ in 0..32 {
      let before = producer.offset;
      producer.append(before, &chunk).unwrap();
      if producer.offset == before {
        break;
      }
    }
    assert!(producer.offset < 1 << 20);
    let before = producer.offset;
    producer.append(before, &chunk).unwrap();
    assert_eq!(producer.offset, before);
    producer.close();
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes.len() as u64, before + 32);
    assert!(!bytes.ends_with(COMMIT));
  }

  #[test]
  fn producer_and_receiver_publish_checked_binary_transfer() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new");
    let bytes = vec![255; 7 * CREATE_LIMIT + 17];
    let mut spec = spec(bytes.len() as u64);
    spec.path = path.to_str().unwrap().into();
    spec.sha256 = format!("{:x}", Sha256::digest(&bytes));
    let (mut producer, mut input) = prepare(&spec).unwrap();
    let target = path.clone();
    let receiver = std::thread::spawn(move || {
      let mut header = [0; 32];
      input.read_exact(&mut header).unwrap();
      let digest = header
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
      crate::files::upload::receive(&target, spec.size, &digest, &mut input)
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while producer.offset < producer.size {
      assert!(std::time::Instant::now() < deadline);
      let offset = producer.offset as usize;
      producer
        .append(
          offset as u64,
          &bytes[offset..bytes.len().min(offset + CREATE_LIMIT)],
        )
        .unwrap();
      std::thread::yield_now();
    }
    while !producer.committed {
      assert!(std::time::Instant::now() < deadline);
      producer.commit().unwrap();
      std::thread::yield_now();
    }
    assert_eq!(receiver.join().unwrap().unwrap().file.length, bytes.len());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
  }

  #[test]
  fn abort_after_all_data_still_cannot_publish() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("new");
    let mut spec = spec(3);
    spec.sha256 = format!("{:x}", Sha256::digest(b"abc"));
    let (mut producer, mut input) = prepare(&spec).unwrap();
    producer.append(0, b"abc").unwrap();
    producer.close();
    let mut header = [0; 32];
    input.read_exact(&mut header).unwrap();
    assert!(crate::files::upload::receive(&path, 3, &spec.sha256, &mut input).is_err());
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
  }
}
