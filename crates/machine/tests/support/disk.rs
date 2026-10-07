use std::{
  fs::{self, OpenOptions},
  io::Write,
  path::PathBuf,
  sync::atomic::{AtomicU64, Ordering},
};

pub struct Image(pub PathBuf);

pub fn image(bytes: &[u8]) -> Image {
  static NEXT: AtomicU64 = AtomicU64::new(0);
  let path = std::env::temp_dir().join(format!(
    "hopperblock-{}-{}",
    std::process::id(),
    NEXT.fetch_add(1, Ordering::Relaxed)
  ));
  let mut file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&path)
    .unwrap();
  file.write_all(bytes).unwrap();
  Image(path)
}

impl Drop for Image {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.0);
  }
}
