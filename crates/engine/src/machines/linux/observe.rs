use super::progress::{self, Decoder, Phase};
use std::{
  fs::File,
  io::Read,
  os::fd::{AsRawFd, FromRawFd},
  path::Path,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
};

pub struct Observation {
  stop: Arc<AtomicBool>,
}

impl Drop for Observation {
  fn drop(&mut self) {
    self.stop.store(true, Ordering::Release);
  }
}

pub fn start(directory: &Path, attempt: &str) -> anyhow::Result<(File, Observation)> {
  start_inner(directory, attempt, None)
}

pub(crate) fn start_owned(
  directory: &Path,
  attempt: &str,
  ownership: Arc<store::lock::Lease>,
) -> anyhow::Result<(File, Observation)> {
  start_inner(directory, attempt, Some(ownership))
}

fn start_inner(
  directory: &Path,
  attempt: &str,
  ownership: Option<Arc<store::lock::Lease>>,
) -> anyhow::Result<(File, Observation)> {
  let mut decoder = Decoder::new(attempt)?;
  progress::save(directory, attempt, Phase::Prepared)?;
  let mut descriptors = [-1; 2];
  if unsafe { libc::pipe(descriptors.as_mut_ptr()) } != 0 {
    return Err(std::io::Error::last_os_error().into());
  }
  let mut input = unsafe { File::from_raw_fd(descriptors[0]) };
  let output = unsafe { File::from_raw_fd(descriptors[1]) };
  for file in [&input, &output] {
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
      return Err(std::io::Error::last_os_error().into());
    }
  }
  let stop = Arc::new(AtomicBool::new(false));
  let flag = stop.clone();
  let directory = directory.to_owned();
  let attempt = attempt.to_owned();
  std::thread::Builder::new()
    .name("ubuntu-install".into())
    .spawn(move || {
      let _ownership = ownership;
      let mut bytes = [0; 8192];
      while !flag.load(Ordering::Acquire) {
        let mut descriptor = libc::pollfd {
          fd: input.as_raw_fd(),
          events: libc::POLLIN,
          revents: 0,
        };
        let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
        if ready < 0 {
          if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
          }
          break;
        }
        if ready == 0 {
          continue;
        }
        match input.read(&mut bytes) {
          Ok(0) => break,
          Ok(count) => {
            for phase in decoder.feed(&bytes[..count]) {
              if flag.load(Ordering::Acquire) {
                break;
              }
              if let Err(error) = progress::save(&directory, &attempt, phase) {
                tracing::error!(%error, "Cannot persist Ubuntu installation progress");
              }
            }
          }
          Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
          Err(_) => break,
        }
      }
    })?;
  Ok((output, Observation { stop }))
}
