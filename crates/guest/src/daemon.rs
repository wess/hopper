pub(crate) mod credentials;
mod socket;

use anyhow::ensure;
use model::guest::wire::{auth::GuestHandshake, CHUNK};
use std::{
  os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
  path::Path,
  sync::atomic::{AtomicBool, Ordering},
  time::{Duration, Instant},
};

const ROOT: &str = "/sys/fs/cgroup/hopper";

fn prepare() -> anyhow::Result<()> {
  let root = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    .open("/sys/fs/cgroup")?;
  let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
  ensure!(
    unsafe { libc::fstatfs(root.as_raw_fd(), &mut filesystem) } == 0
      && filesystem.f_type as u64 == 0x63677270,
    "Guest service requires cgroup v2"
  );
  let result = unsafe { libc::mkdirat(root.as_raw_fd(), c"hopper".as_ptr(), 0o700) };
  if result != 0 {
    let error = std::io::Error::last_os_error();
    ensure!(
      error.kind() == std::io::ErrorKind::AlreadyExists,
      "Create guest cgroup root: {error}"
    );
  }
  Ok(())
}

fn connection(stop: &AtomicBool) -> anyhow::Result<()> {
  let material = credentials::load()?;
  let socket = socket::connect(stop)?;
  let deadline = Instant::now() + Duration::from_secs(10);
  let mut hello = [0; 101];
  socket.exact(&mut hello, stop, deadline)?;
  let (handshake, answer) = GuestHandshake::answer(material.key, &material.boot, &hello)?;
  socket.write(&answer, stop, deadline)?;
  let mut confirmation = [0; 36];
  socket.exact(&mut confirmation, stop, deadline)?;
  let session = handshake.finish(&confirmation)?;
  prepare()?;
  let mut service = crate::service::open(session, Path::new(ROOT))?;
  let mut bytes = [0; CHUNK];
  let mut deadline = None;
  let result = (|| -> anyhow::Result<()> {
    loop {
      let count = socket.read(&mut bytes, stop, deadline)?;
      if count == 0 {
        return Ok(());
      }
      let started = *deadline.get_or_insert_with(|| Instant::now() + Duration::from_secs(5));
      socket::check(stop, Some(started))?;
      if let Some(reply) = service.feed(&bytes[..count])? {
        socket.write(&reply, stop, started)?;
        deadline = None;
      }
    }
  })();
  service.close();
  let began = Instant::now();
  let mut reported = false;
  while !service.drained() {
    if !reported && began.elapsed() >= Duration::from_secs(5) {
      eprintln!("Guest service is waiting for process cleanup");
      reported = true;
    }
    std::thread::sleep(Duration::from_millis(100));
  }
  result
}

pub fn run(stop: &AtomicBool) -> anyhow::Result<()> {
  ensure!(
    unsafe {
      libc::getuid() == 0 && libc::geteuid() == 0 && libc::getgid() == 0 && libc::getegid() == 0
    },
    "Guest service requires root"
  );
  while !stop.load(Ordering::Acquire) {
    if let Err(error) = connection(stop) {
      if stop.load(Ordering::Acquire) {
        break;
      }
      eprintln!(
        "Guest connection unavailable: {}",
        crate::jobs::error(error)
      );
    }
    let retry = Instant::now() + Duration::from_secs(2);
    while !stop.load(Ordering::Acquire) && Instant::now() < retry {
      std::thread::sleep(Duration::from_millis(100));
    }
  }
  Ok(())
}
