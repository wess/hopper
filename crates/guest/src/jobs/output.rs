use super::Control;
use std::{
  io::Read,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};

const LIMIT: usize = 64 << 10;

pub(super) fn drain(
  job: Arc<Control>,
  mut pipe: impl Read + Send + 'static,
  stderr: bool,
  stop: Arc<AtomicBool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
  std::thread::Builder::new()
    .name("hopperoutput".into())
    .spawn(move || {
      let mut bytes = [0; 8192];
      let mut deadline = None;
      loop {
        if stop.load(Ordering::Acquire) && deadline.is_none() {
          deadline = Some(Instant::now() + Duration::from_millis(250));
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
          break;
        }
        match pipe.read(&mut bytes) {
          Ok(0) => break,
          Ok(count) => {
            let mut result = job.result.lock().unwrap();
            let (output, truncated) = if stderr {
              let result = &mut *result;
              (&mut result.stderr, &mut result.stderr_truncated)
            } else {
              let result = &mut *result;
              (&mut result.stdout, &mut result.stdout_truncated)
            };
            let accepted = count.min(LIMIT - output.len());
            output.extend_from_slice(&bytes[..accepted]);
            *truncated |= accepted < count;
          }
          Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
          Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            std::thread::sleep(Duration::from_millis(10))
          }
          Err(error) => {
            job.result.lock().unwrap().error = Some(super::error(error));
            break;
          }
        }
      }
    })
}
