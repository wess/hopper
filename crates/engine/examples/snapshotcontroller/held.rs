use anyhow::{ensure, Context};
use engine::machines::native::{self, config, restore::Owner, State};
use machine::{
  runtime::hardware,
  snapshot::{checkpoint, manifest::Binding, package::Package},
};
use model::native::{snapshot::Status, Boot, Command, Restore, Result as Reply};
use std::{
  fs::File,
  os::{
    fd::{AsRawFd, FromRawFd},
    unix::fs::FileExt,
  },
  sync::Arc,
  time::Duration,
};

pub async fn verify(
  worker: &native::Client,
  owner: &Arc<Owner>,
  paths: &config::Paths,
  boot: &Boot,
  restore: &Restore,
) -> anyhow::Result<()> {
  let selected = owner
    .selected()?
    .context("Missing held current selection")?;
  let parent = File::open(paths.root.join("snapshots"))?;
  let source = Package::inspect(&parent, selected.machine, selected.snapshot, || {
    owner.check()
  })?;
  let layout = source.layout();
  let snapshot = model::new_uuid();
  owner.check()?;
  ensure!(matches!(
    native::request(
      worker,
      Command::SnapshotStart {
        snapshot_id: snapshot.clone(),
        policy_generation: restore.policy_generation,
      }
    )
    .await?,
    Reply::Accepted {}
  ));
  let receipt = tokio::time::timeout(Duration::from_secs(60), async {
    loop {
      owner.check()?;
      let Reply::Snapshot { status, .. } = native::request(
        worker,
        Command::SnapshotPoll {
          snapshot_id: snapshot.clone(),
          policy_generation: restore.policy_generation,
        },
      )
      .await?
      else {
        anyhow::bail!("Held capture returned no status");
      };
      match status {
        Status::Ready { receipt } => break Ok(receipt),
        Status::Failed {} => anyhow::bail!("Held capture failed"),
        _ => tokio::time::sleep(Duration::from_millis(10)).await,
      }
    }
  })
  .await??;
  let policy = owner.policy();
  let binding = Binding {
    snapshot: model::native::machine_identity(&snapshot).unwrap(),
    machine: selected.machine,
    configuration: selected.configuration,
  };
  owner.check()?;
  let package = policy.publish(&parent, receipt, binding, layout)?;
  owner.check()?;
  ensure!(matches!(
    native::request(
      worker,
      Command::SnapshotCancel {
        snapshot_id: snapshot,
        policy_generation: restore.policy_generation,
      }
    )
    .await?,
    Reply::Accepted {}
  ));
  owner.check()?;
  ensure!(matches!(native::state(worker), State::Paused));
  ensure!(matches!(
    native::request(worker, Command::Status {}).await?,
    Reply::Status { paused: true, .. }
  ));
  ensure!(native::request(worker, Command::Resume {}).await.is_err());
  let (_, hardware, _) =
    hardware::checkpoint(boot.clone(), &package, selected.configuration, || {
      owner.check()
    })?;
  let saved = checkpoint::load(&package, &hardware, || owner.check())?;
  let mut bytes = [0; 4096];
  saved.primary.memory.ram.read(1 << 20, &mut bytes)?;
  ensure!(
    bytes == [0; 4096],
    "Held cold capture executed guest RAM-writing firmware"
  );
  saved.primary.memory.nvram.read(0, &mut bytes[..64])?;
  ensure!(
    bytes[..64] == [0x7f; 64],
    "Held capture lost current EFI variables"
  );
  let image = package.image(4, || owner.check())?;
  let fd = unsafe {
    libc::openat(
      image.as_raw_fd(),
      c"image".as_ptr(),
      libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
    )
  };
  ensure!(fd >= 0, "Open held checkpoint image");
  let file = unsafe { File::from_raw_fd(fd) };
  file.read_exact_at(&mut bytes[..23], 0)?;
  ensure!(
    &bytes[..23] == b"current generation disk",
    "Held capture lost current disk bytes"
  );
  println!("Signed held current-generation capture preserves current disk/EFI and zero guest RAM before RAM-writing firmware executes, fully admits its checkpoint and remains paused without ordinary resume authorization");
  Ok(())
}
