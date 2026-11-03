use super::Policy;
use anyhow::ensure;
use std::{
  ffi::CStr,
  fs::{File, OpenOptions},
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
  },
};

/// Saved manifest claims, not proof that the checkpoint can restore on current hardware.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
  pub id: String,
  pub machine_id: String,
  pub configuration: String,
  pub cpus: u8,
  pub memory_bytes: u64,
  pub name: Option<String>,
  pub created_unix_seconds: Option<u64>,
}

impl Policy {
  /// Durable discovery needs no worker. Missing namespaces are empty; malformed ones reject.
  pub fn inventory(&self) -> anyhow::Result<Vec<Entry>> {
    self.check()?;
    let manager = &self.0.manager;
    let root = OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
      .open(&manager.root)?;
    let Some(native) = open(&root, c"native")? else {
      self.check()?;
      return Ok(Vec::new());
    };
    let name = std::ffi::CString::new(self.0.id.as_str())?;
    let Some(vm) = open(&native, &name)? else {
      self.check()?;
      same(&root, c"native", &native)?;
      return Ok(Vec::new());
    };
    let entries = match open(&vm, c"snapshots")? {
      Some(parent) => {
        let identity = model::native::machine_identity(&self.0.id)
          .ok_or_else(|| anyhow::anyhow!("Invalid snapshot machine identity"))?;
        let entries = machine::snapshot::package::inventory(&parent, identity, || self.check())?;
        same(&vm, c"snapshots", &parent)?;
        entries
          .into_iter()
          .map(|entry| Entry {
            id: uuid::Uuid::from_bytes(entry.binding.snapshot).to_string(),
            machine_id: self.0.id.clone(),
            configuration: entry
              .binding
              .configuration
              .iter()
              .map(|byte| format!("{byte:02x}"))
              .collect(),
            name: entry
              .description
              .as_ref()
              .map(|description| description.name().into()),
            created_unix_seconds: entry
              .description
              .as_ref()
              .map(|description| description.created()),
            cpus: entry.layout.cpus,
            memory_bytes: entry.layout.banks[2].1,
          })
          .collect()
      }
      None => Vec::new(),
    };
    same(&native, &name, &vm)?;
    same(&root, c"native", &native)?;
    self.check()?;
    Ok(entries)
  }
}

fn open(parent: &File, name: &CStr) -> anyhow::Result<Option<File>> {
  let fd = unsafe {
    libc::openat(
      parent.as_raw_fd(),
      name.as_ptr(),
      libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  if fd < 0 {
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOENT) {
      return Ok(None);
    }
    return Err(error.into());
  }
  let file = unsafe { File::from_raw_fd(fd) };
  let meta = file.metadata()?;
  ensure!(
    meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o7777 == 0o700,
    "Saved snapshot namespace is not private"
  );
  Ok(Some(file))
}

fn same(parent: &File, name: &CStr, pinned: &File) -> anyhow::Result<()> {
  let current = open(parent, name)?
    .ok_or_else(|| anyhow::anyhow!("Saved snapshot namespace disappeared"))?
    .metadata()?;
  let original = pinned.metadata()?;
  ensure!(
    (current.dev(), current.ino()) == (original.dev(), original.ino()),
    "Saved snapshot namespace was replaced"
  );
  Ok(())
}
