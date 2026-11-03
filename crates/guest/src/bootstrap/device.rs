use anyhow::{ensure, Context};
use model::guest::wire::auth::Material;
use std::{
  fs::{File, OpenOptions},
  io::{Read, Write},
  os::{
    fd::AsRawFd,
    unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
  },
  sync::atomic::{AtomicBool, Ordering},
  time::{Duration, Instant},
};
use zeroize::Zeroizing;

fn attribute(path: &std::path::Path) -> anyhow::Result<String> {
  let file = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(path)
    .with_context(|| format!("Open boot device attribute {}", path.display()))?;
  let mut value = String::new();
  file.take(129).read_to_string(&mut value)?;
  ensure!(
    value.len() <= 128 && !value.contains('\0'),
    "Invalid boot device attribute"
  );
  Ok(value)
}

fn open() -> anyhow::Result<Option<File>> {
  let path = std::path::Path::new("/sys/class/virtio-ports");
  if !path.exists() {
    return Ok(None);
  }
  let directory = OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
    .open(path)?;
  let mut filesystem: libc::statfs = unsafe { std::mem::zeroed() };
  ensure!(
    unsafe { libc::fstatfs(directory.as_raw_fd(), &mut filesystem) } == 0
      && filesystem.f_type as u64 == 0x62656572,
    "Boot device discovery requires sysfs"
  );
  let entries = std::fs::read_dir(path)?
    .take(65)
    .collect::<std::io::Result<Vec<_>>>()?;
  ensure!(entries.len() <= 64, "Too many virtio ports");
  let mut found = None;
  for entry in entries {
    let name = entry.file_name();
    let name = name
      .to_str()
      .ok_or_else(|| anyhow::anyhow!("Invalid virtio device name"))?;
    ensure!(super::port(name), "Invalid virtio device name");
    let port_name = match attribute(&entry.path().join("name")) {
      Ok(name) => name,
      Err(error)
        if error
          .downcast_ref::<std::io::Error>()
          .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
      {
        continue
      }
      Err(error) => return Err(error),
    };
    if port_name != "hopper.boot\n" {
      continue;
    }
    ensure!(found.is_none(), "Ambiguous private boot port");
    let identity = super::number(&attribute(&entry.path().join("dev"))?)?;
    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
      .open(std::path::Path::new("/dev").join(name))
      .context("Open named private boot device")?;
    protected(&file, identity)?;
    found = Some((file, identity));
  }
  Ok(found.map(|(file, _)| file))
}

fn protected(file: &File, identity: (u32, u32)) -> anyhow::Result<()> {
  let info = file.metadata()?;
  ensure!(
    info.file_type().is_char_device()
      && info.uid() == 0
      && info.gid() == 0
      && info.mode() & 0o7777 == 0o600
      && libc::major(info.rdev()) == identity.0
      && libc::minor(info.rdev()) == identity.1,
    "Boot device is not private or has the wrong identity"
  );
  Ok(())
}

pub(super) fn read(stop: &AtomicBool) -> anyhow::Result<Material> {
  let deadline = Instant::now() + Duration::from_secs(180);
  let mut file = loop {
    check(stop, deadline)?;
    if let Some(file) = open()? {
      break file;
    }
    std::thread::sleep(Duration::from_millis(50));
  };
  let identity = file.metadata()?.rdev();
  let identity = (libc::major(identity), libc::minor(identity));
  let mut sent = 0;
  while sent < Material::REQUEST.len() {
    check(stop, deadline)?;
    protected(&file, identity)?;
    match file.write(&Material::REQUEST[sent..]) {
      Ok(0) => std::thread::sleep(Duration::from_millis(50)),
      Ok(count) => sent += count,
      Err(error)
        if matches!(
          error.kind(),
          std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        ) =>
      {
        std::thread::sleep(Duration::from_millis(50))
      }
      Err(error) => return Err(error.into()),
    }
  }
  let mut bytes = Zeroizing::new([0; Material::LENGTH]);
  let mut count = 0;
  while count < bytes.len() {
    check(stop, deadline)?;
    protected(&file, identity)?;
    match file.read(&mut bytes[count..]) {
      Ok(0) => std::thread::sleep(Duration::from_millis(50)),
      Ok(read) => count += read,
      Err(error)
        if matches!(
          error.kind(),
          std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
        ) =>
      {
        std::thread::sleep(Duration::from_millis(50))
      }
      Err(error) => return Err(error.into()),
    }
  }
  check(stop, deadline)?;
  protected(&file, identity)?;
  let material = Material::read(&mut &bytes[..])?;
  ensure!(
    material.boot.guest == model::GuestOs::Linux,
    "Boot input requires Linux"
  );
  Ok(material)
}

fn check(stop: &AtomicBool, deadline: Instant) -> anyhow::Result<()> {
  ensure!(!stop.load(Ordering::Acquire), "Boot delivery cancelled");
  ensure!(Instant::now() < deadline, "Private boot input timed out");
  Ok(())
}
