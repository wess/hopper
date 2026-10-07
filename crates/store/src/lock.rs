use fs2::FileExt;
use std::{fs::File, io, ops::Deref};

pub struct Lease(File);

pub fn exclusive(file: File) -> io::Result<Lease> {
  file.try_lock_exclusive()?;
  Ok(Lease(file))
}

impl Deref for Lease {
  type Target = File;

  fn deref(&self) -> &File {
    &self.0
  }
}

impl Drop for Lease {
  fn drop(&mut self) {
    // close alone retains flock while another process still has an inherited descriptor.
    let _ = FileExt::unlock(&self.0);
  }
}
