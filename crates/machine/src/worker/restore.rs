use super::{poll::Parent, snapshot};
use anyhow::{ensure, Context};
use machine::{
  runtime::{self, Mode},
  snapshot::{
    checkpoint,
    generation::Generation,
    package::Package,
    selection::{self, Phase, Record},
  },
};
use model::native::{Boot, Command, Restore, Result as Reply, StopReason};
use std::{
  fs::File,
  os::unix::fs::{MetadataExt, OpenOptionsExt},
  path::Path,
  time::{Duration, Instant},
};

pub(super) struct Gate {
  restore: Restore,
  root: File,
  active: Record,
  deadline: Instant,
  capture_deadline: Option<Instant>,
}

impl Gate {
  pub(super) fn active(root: File, restore: Restore, active: Record) -> anyhow::Result<Self> {
    ensure!(
      active.phase == Phase::Active
        && Some(active.snapshot) == model::native::machine_identity(&restore.snapshot_id)
        && Some(active.generation) == model::native::machine_identity(&restore.generation_id),
      "Generation resume binding differs"
    );
    active.encode()?;
    Ok(Self {
      root,
      restore,
      active,
      deadline: Instant::now() + Duration::from_secs(30),
      capture_deadline: Some(Instant::now() + Duration::from_secs(300)),
    })
  }

  pub fn check(&self) -> anyhow::Result<()> {
    ensure!(
      Instant::now() < self.deadline,
      "Restored admission authorization expired"
    );
    Ok(())
  }

  pub(super) fn blocks(&self, command: &Command) -> bool {
    matches!(
      command,
      Command::Resume {}
        | Command::Input { .. }
        | Command::Release {}
        | Command::FinishDeployment {}
    ) || self.capture_deadline.is_none() && matches!(command, Command::SnapshotStart { .. })
  }

  pub(super) fn begin_capture(&mut self) -> anyhow::Result<()> {
    self.check()?;
    self.deadline = self
      .capture_deadline
      .context("Checkpoint handoff cannot capture before activation")?;
    Ok(())
  }

  pub fn resume(&self, restore: &Restore) -> anyhow::Result<()> {
    self.check()?;
    ensure!(
      restore == &self.restore,
      "Restore resume identity/policy differs from admission"
    );
    ensure!(
      selection::read_at(&self.root, self.active.machine)? == self.active,
      "Restore generation is not durably active"
    );
    Ok(())
  }
}

pub(super) fn activate(
  parent: &mut Parent,
  id: u64,
  restore: Restore,
  paused: bool,
) -> anyhow::Result<()> {
  let authorized = parent
    .restore
    .as_ref()
    .context("No restored admission is pending")
    .and_then(|gate| gate.resume(&restore));
  match authorized {
    Ok(()) => {
      parent.restore = None;
      parent.held = paused;
      parent.reply(
        id,
        if paused {
          Reply::Paused {}
        } else {
          Reply::Running {}
        },
        &[],
      )
    }
    Err(error) => parent.reply(
      id,
      Reply::Rejected {
        message: error.to_string(),
      },
      &[],
    ),
  }
}

pub(super) fn directory(path: &Path) -> anyhow::Result<File> {
  let file = std::fs::OpenOptions::new()
    .read(true)
    .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
    .open(path)?;
  let m = file.metadata()?;
  ensure!(
    m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o7777 == 0o700,
    "Restore namespace is not private"
  );
  Ok(file)
}

pub(super) fn run(
  mut parent: Parent,
  id: u64,
  config: Boot,
  restore: Restore,
) -> anyhow::Result<()> {
  let snapshot = model::native::machine_identity(&restore.snapshot_id)
    .context("Invalid restored snapshot identity")?;
  let generation = model::native::machine_identity(&restore.generation_id)
    .context("Invalid restored generation identity")?;
  let machine_id = config.machine_id.clone();
  let store_path = config.store.clone();
  let root = Path::new(&store_path)
    .parent()
    .context("Missing original VM namespace")?;
  ensure!(
    root.is_absolute()
      && Path::new(&store_path).file_name() == Some(std::ffi::OsStr::new("variables")),
    "Restore requires the original VM namespace"
  );
  let root = directory(root)?;
  let machine =
    model::native::machine_identity(&machine_id).context("Invalid restored VM identity")?;
  let selected = selection::read_at(&root, machine)?;
  ensure!(
    selected.phase == Phase::Attempted
      && selected.generation == generation
      && selected.snapshot == snapshot,
    "Restored worker differs from host attempt selection"
  );
  let child = |name: &std::ffi::CStr| -> anyhow::Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let fd = unsafe {
      libc::openat(
        root.as_raw_fd(),
        name.as_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
      )
    };
    ensure!(
      fd >= 0,
      "Open restored child namespace: {}",
      std::io::Error::last_os_error()
    );
    let file = unsafe { File::from_raw_fd(fd) };
    let m = file.metadata()?;
    ensure!(
      m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o7777 == 0o700,
      "Restored child namespace is not private"
    );
    Ok(file)
  };
  let package = Package::inspect(&child(c"snapshots")?, machine, snapshot, || Ok(()))?;
  let (boot, devices, configuration) =
    runtime::hardware::checkpoint(config, &package, selected.configuration, || Ok(()))?;
  let layout = package.layout();
  let ready = Generation::open(
    &child(c"generations")?,
    &package,
    &devices,
    generation,
    || Ok(()),
  )?;
  // saved policy/operation grants belong to the parent; only it can authorize the held CPUs.
  let (saved, mut store, disks) = ready.consume(|| Ok(()), || Ok(()))?;
  ensure!(
    checkpoint::layout(&saved)? == layout,
    "Restored checkpoint topology changed"
  );
  let captures = machine::hypervisor::state::group::create(boot.cpus as usize)?;
  parent.snapshots = Some(snapshot::Snapshots::new(
    &store_path,
    &machine_id,
    configuration,
    captures.clone(),
  )?);
  parent.held = true;
  parent.restore = Some(Gate {
    restore: restore.clone(),
    root: root.try_clone()?,
    active: Record {
      phase: Phase::Active,
      ..selected
    },
    deadline: Instant::now() + Duration::from_secs(30),
    capture_deadline: None,
  });
  let mut initialized = false;
  let stopped = runtime::recovery::run_recovered_captured(
    boot,
    devices,
    &mut store,
    &saved,
    disks,
    captures,
    |devices, ram, mode| {
      if !initialized {
        ensure!(mode == Mode::Paused, "Restore did not start held");
        parent.reply(
          id,
          Reply::Restored {
            machine_id: machine_id.clone(),
            configuration,
            restore: restore.clone(),
          },
          &[],
        )?;
        initialized = true;
      }
      parent.poll(devices, ram, mode)
    },
  )?;
  ensure!(
    runtime::variables::bytes(&store) == stopped.variables,
    "Restored firmware persistence differs from stopped guest"
  );
  let reason = match stopped.reason {
    runtime::Reason::Stopped => StopReason::Requested,
    runtime::Reason::Shutdown => StopReason::Shutdown,
    runtime::Reason::Reset => StopReason::Reset,
  };
  super::drain(&mut parent)?;
  parent.reply(parent.stopping.unwrap_or(0), Reply::Stopped { reason }, &[])?;
  super::drain(&mut parent)?;
  Ok(())
}
