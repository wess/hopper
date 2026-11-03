use super::{send, Stream};
use anyhow::{ensure, Context};
use model::native::Response;
use std::{
  fs::File,
  os::fd::AsRawFd,
  sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender, TryRecvError},
    Arc,
  },
  thread::JoinHandle,
  time::{Duration, Instant},
};

/// one reply in flight; only the pipe writer leaves the VM owner thread.
pub struct Output {
  requests: Option<SyncSender<Stream>>,
  replies: Receiver<anyhow::Result<()>>,
  cancelled: Arc<AtomicBool>,
  worker: Option<JoinHandle<()>>,
  busy: bool,
}

impl Output {
  pub fn new(mut file: File) -> anyhow::Result<Self> {
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    ensure!(flags >= 0, "Read native output flags");
    ensure!(
      flags & libc::O_NONBLOCK != 0,
      "Native output must be nonblocking"
    );
    let (requests, incoming) = mpsc::sync_channel::<Stream>(1);
    let (completed, replies) = mpsc::sync_channel(1);
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let worker = std::thread::Builder::new()
      .name("hopper output".into())
      .spawn(move || {
        while let Ok(mut stream) = incoming.recv() {
          let result = drain(&mut stream, &mut file, &stop);
          drop(stream);
          let failed = result.is_err();
          if completed.send(result).is_err() || failed {
            break;
          }
        }
      })
      .context("Start native pipe writer")?;
    Ok(Self {
      requests: Some(requests),
      replies,
      cancelled,
      worker: Some(worker),
      busy: false,
    })
  }

  pub fn send(&mut self, response: &Response, payload: &[u8]) -> anyhow::Result<()> {
    ensure!(!self.pending()?, "Native response is still pending");
    let mut stream = Stream::default();
    send(&mut stream, response, payload)?;
    self
      .requests
      .as_ref()
      .context("Native pipe writer closed")?
      .try_send(stream)
      .map_err(|_| anyhow::anyhow!("Native pipe writer unavailable"))?;
    self.busy = true;
    Ok(())
  }

  pub fn pending(&mut self) -> anyhow::Result<bool> {
    if self.busy {
      match self.replies.try_recv() {
        Ok(result) => {
          result?;
          self.busy = false;
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => anyhow::bail!("Native pipe writer stopped"),
      }
    }
    Ok(self.busy)
  }
}

impl Drop for Output {
  fn drop(&mut self) {
    self.cancelled.store(true, Ordering::Release);
    self.requests.take();
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
  }
}

fn drain(stream: &mut Stream, file: &mut File, cancelled: &AtomicBool) -> anyhow::Result<()> {
  let mut progress = Instant::now();
  while stream.pending() {
    ensure!(
      !cancelled.load(Ordering::Acquire),
      "Native pipe writer cancelled"
    );
    ensure!(
      progress.elapsed() < Duration::from_secs(5),
      "Native parent stopped receiving replies"
    );
    if stream.flush(file)? > 0 {
      progress = Instant::now();
      continue;
    }
    // readiness wakes immediately when the parent drains; the timeout bounds cancellation.
    let mut fd = libc::pollfd {
      fd: file.as_raw_fd(),
      events: libc::POLLOUT,
      revents: 0,
    };
    let ready = unsafe { libc::poll(&mut fd, 1, 20) };
    if ready < 0 {
      let error = std::io::Error::last_os_error();
      if error.kind() != std::io::ErrorKind::Interrupted {
        return Err(error.into());
      }
    }
    ensure!(
      fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0,
      "Native parent disconnected from replies"
    );
  }
  Ok(())
}
