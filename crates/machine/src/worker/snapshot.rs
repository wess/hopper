mod authorization;
use anyhow::{ensure, Context};
use authorization::Grant;
use machine::{
  hypervisor::state::group::{Capture, Group},
  snapshot::{
    manifest::Binding,
    package::Sealed,
    publication::{self, Publication},
  },
};
use model::native::snapshot::Status;
use std::{
  fs::File,
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::{MetadataExt, OpenOptionsExt},
  },
  path::Path,
  sync::Arc,
  time::Duration,
};

pub(super) struct Snapshots {
  root: File,
  binding: Binding,
  group: Arc<Group>,
  active: Option<Job>,
}

struct Job {
  id: String,
  policy: u64,
  grant: Arc<Grant>,
  parent: File,
  phase: Phase,
}

enum Phase {
  Capturing(Capture),
  Staging(Publication<File, Sealed>),
  Ready(Box<Sealed>),
  Failed,
}

impl Snapshots {
  pub fn new(
    store: &str,
    id: &str,
    configuration: [u8; 32],
    group: Arc<Group>,
  ) -> anyhow::Result<Self> {
    let path = Path::new(store)
      .parent()
      .context("Snapshot namespace has no parent")?;
    ensure!(path.is_absolute(), "Snapshot namespace must be absolute");
    let root = std::fs::OpenOptions::new()
      .read(true)
      .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
      .open(path)?;
    private(&root)?;
    Ok(Self {
      root,
      binding: Binding {
        machine: model::native::machine_identity(id).context("Invalid snapshot VM identity")?,
        snapshot: [0; 16],
        configuration,
      },
      group,
      active: None,
    })
  }

  pub fn start(&mut self, id: String, policy: u64) -> anyhow::Result<()> {
    ensure!(self.active.is_none(), "A snapshot job is already retained");
    let snapshot = model::native::machine_identity(&id).context("Invalid snapshot identity")?;
    let parent = namespace(&self.root)?;
    let capture = self.group.request()?;
    self.binding.snapshot = snapshot;
    self.active = Some(Job {
      id,
      policy,
      grant: Arc::new(Grant::new()),
      parent,
      phase: Phase::Capturing(capture),
    });
    Ok(())
  }

  pub fn capturing(&self) -> bool {
    self
      .active
      .as_ref()
      .is_some_and(|job| matches!(job.phase, Phase::Capturing(_)))
  }

  pub fn tick(&mut self) {
    if let Some(job) = &mut self.active {
      if job.advance(self.binding).is_err() {
        job.grant.cancel();
        job.phase = Phase::Failed;
      }
    }
  }

  pub fn poll(&self, id: &str, policy: u64) -> anyhow::Result<Status> {
    let job = self
      .active
      .as_ref()
      .context("Snapshot job is unavailable")?;
    ensure!(
      job.id == id && job.policy == policy,
      "Snapshot job identity or policy differs"
    );
    if matches!(job.phase, Phase::Failed) {
      return Ok(Status::Failed {});
    }
    job.grant.renew()?;
    Ok(match &job.phase {
      Phase::Capturing(_) => Status::Capturing {},
      Phase::Staging(_) => Status::Staging {},
      Phase::Ready(sealed) => Status::Ready {
        receipt: sealed.receipt(),
      },
      Phase::Failed => Status::Failed {},
    })
  }

  pub fn cancel(&mut self, id: &str, policy: u64) -> anyhow::Result<()> {
    ensure!(
      model::native::machine_identity(id).is_some(),
      "Invalid snapshot identity"
    );
    if self.active.is_none() {
      return Ok(());
    }
    let job = self
      .active
      .as_ref()
      .context("Snapshot job is unavailable")?;
    ensure!(
      job.id == id && job.policy == policy,
      "Snapshot job identity or policy differs"
    );
    self.clear();
    Ok(())
  }

  pub fn clear(&mut self) {
    if let Some(job) = self.active.take() {
      job.grant.cancel();
      drop(job);
    }
  }
}

impl Drop for Snapshots {
  fn drop(&mut self) {
    self.clear();
  }
}

impl Job {
  fn advance(&mut self, binding: Binding) -> anyhow::Result<()> {
    if matches!(self.phase, Phase::Failed) {
      return Ok(());
    }
    self.grant.check()?;
    match &mut self.phase {
      Phase::Capturing(capture) => {
        if let Some(saved) = capture.poll()? {
          let grant = self.grant.clone();
          self.phase = Phase::Staging(publication::stage(
            saved,
            self.parent.try_clone()?,
            binding,
            self.parent.try_clone()?,
            Duration::from_secs(300),
            Arc::new(move || grant.check()),
          )?);
        }
      }
      Phase::Staging(writer) => {
        if let Some(sealed) = writer.poll()? {
          self.phase = Phase::Ready(Box::new(sealed));
        }
      }
      Phase::Ready(_) | Phase::Failed => {}
    }
    Ok(())
  }
}

fn private(file: &File) -> anyhow::Result<()> {
  let meta = file.metadata()?;
  ensure!(
    meta.is_dir() && meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o7777 == 0o700,
    "Snapshot namespace is not private"
  );
  Ok(())
}

fn namespace(root: &File) -> anyhow::Result<File> {
  private(root)?;
  let result = unsafe { libc::mkdirat(root.as_raw_fd(), c"snapshots".as_ptr(), 0o700) };
  ensure!(
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST),
    "Create snapshot namespace: {}",
    std::io::Error::last_os_error()
  );
  let fd = unsafe {
    libc::openat(
      root.as_raw_fd(),
      c"snapshots".as_ptr(),
      libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
    )
  };
  ensure!(
    fd >= 0,
    "Open snapshot namespace: {}",
    std::io::Error::last_os_error()
  );
  let file = unsafe { File::from_raw_fd(fd) };
  private(&file)?;
  Ok(file)
}
