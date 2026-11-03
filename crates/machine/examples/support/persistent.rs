use anyhow::{ensure, Context};
use machine::{
  devices::virtio::pci,
  hypervisor::state::group::Checkpoint,
  runtime::{self, recovery::Disks, variables, Boot, Control, Mode},
};
use std::{
  fs::{File, OpenOptions, Permissions},
  os::unix::fs::{FileExt, PermissionsExt},
};

pub fn run(
  saved: &Checkpoint,
  boot: &impl Fn() -> Boot,
  devices: impl Fn() -> anyhow::Result<[Option<pci::Device>; 7]>,
) -> anyhow::Result<()> {
  reject(saved, boot, &devices)?;
  super::persistentcapture::run(saved, boot, &devices)?;
  Ok(())
}

fn reject(
  saved: &Checkpoint,
  boot: &impl Fn() -> Boot,
  devices: &impl Fn() -> anyhow::Result<[Option<pci::Device>; 7]>,
) -> anyhow::Result<()> {
  use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  };
  for fault in 0..5 {
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), Permissions::from_mode(0o700))?;
    let parent = File::open(directory.path())?;
    let state = saved.primary.devices.transports[0]
      .as_ref()
      .context("Missing disk")?;
    let disk = if fault == 2 {
      let foreign = tempfile::tempfile()?;
      foreign.set_len(64 << 20)?;
      machine::snapshot::storage::capture(&foreign)?.prepare(&parent, || Ok(()))?
    } else {
      pci::snapshot::prepare_disk(state, &parent, || Ok(()))?
    };
    if fault == 1 {
      disk.file().write_all_at(b"modified", 0)?;
    }
    if fault == 3 {
      std::fs::rename(
        directory.path().join("disk"),
        directory.path().join("preserved"),
      )?;
      let replacement = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.path().join("disk"))?;
      replacement.set_len(64 << 20)?;
      replacement.set_permissions(Permissions::from_mode(0o600))?;
    }
    let mut store = variables::from_snapshot(
      &directory.path().join("variables"),
      &saved.primary.memory.nvram,
      &saved.primary.devices.flash,
      || Ok(()),
    )?;
    let mut files = std::array::from_fn(|_| None);
    if fault == 0 {
      drop(disk);
    } else {
      files[0] = Some(disk);
    }
    let allowed = Arc::new(AtomicBool::new(true));
    let original = allowed.clone();
    let disks = Disks {
      files,
      authorize: Box::new(move || {
        ensure!(
          original.load(Ordering::Acquire),
          "Persistent recovery policy revoked"
        );
        Ok(())
      }),
    };
    let result = runtime::recovery::run_recovered(
      boot(),
      devices()?,
      &mut store,
      saved,
      disks,
      |_, _, mode| {
        ensure!(
          fault == 4 && mode == Mode::Paused,
          "Invalid disk reached a guest callback"
        );
        allowed.store(false, Ordering::Release);
        Ok(Control::Continue)
      },
    );
    let error = result
      .err()
      .context("Invalid persistent disk was accepted")?;
    let expected = [
      "exactly the captured disk slots",
      "differs from captured contents",
      "belongs to another capture",
      "namespace was replaced",
      "policy revoked",
    ][fault];
    ensure!(
      error.to_string().contains(expected),
      "Wrong persistent disk rejection: {error:#}"
    );
    let path = directory
      .path()
      .join(if fault == 3 { "preserved" } else { "disk" });
    let contender = OpenOptions::new().read(true).write(true).open(path)?;
    fs2::FileExt::try_lock_exclusive(&contender)?;
    fs2::FileExt::unlock(&contender)?;
  }
  println!("Persistent recovery rejects missing/changed/foreign/replaced disks before callbacks and revocation before Continue, releasing prepared ownership on every failure");
  Ok(())
}
