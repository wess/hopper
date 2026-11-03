#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "vzcommands/files.rs"]
mod files;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn main() -> anyhow::Result<()> {
  let root = tempfile::tempdir()?;
  let result = run(root.path());
  if result.is_err() {
    eprintln!(
      "Command probe evidence retained at {}",
      root.keep().display()
    );
  }
  result
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn run(root: &std::path::Path) -> anyhow::Result<()> {
  use anyhow::{ensure, Context};
  use machine::vz::{
    self,
    queue::{Check, Owner},
    Action, Boot, Linux,
  };
  use model::guest::{
    wire::{Action as Request, Outcome},
    Command, Job, Role,
  };
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use std::{
    os::unix::fs::OpenOptionsExt,
    sync::Arc,
    time::{Duration, Instant},
  };

  let args = std::env::args_os().skip(1).collect::<Vec<_>>();
  ensure!(
    args.len() == 3,
    "Provide verified kernel, production daemon initramfs and matching modloop"
  );
  let main = vz::MainThreadMarker::new().context("Command probe requires the main thread")?;
  let disk = root.join("disk");
  std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&disk)?
    .set_len(1 << 30)?;
  let output = root.join("console");
  let console = std::fs::OpenOptions::new()
    .create_new(true)
    .write(true)
    .mode(0o600)
    .open(&output)?;
  let boot = Linux {
    cpus: 2,
    memory: 512 << 20,
    width: 1024,
    height: 768,
    identity: vz::identity(),
    boot: Boot::Kernel {
      kernel: args[0].clone().into(),
      initramfs: Some(args[1].clone().into()),
      command_line: "console=hvc0 rdinit=/init".into(),
    },
    disk,
    installer: Some(args[2].clone().into()),
    seed: None,
    network: None,
    console: Some(console),
    shares: Vec::new(),
    speakers: false,
  };
  let id = "f196c47e-bd9e-4e84-a49c-354b95091819";
  let check: Check = Arc::new(|| Ok(()));
  let (client, mut owner) = vz::queue::channel();
  let mut vm = vz::bootstrap::provision(main, &boot, id, owner.next_generation()?, &check)?;
  let held = Arc::new(());
  let weak = Arc::downgrade(&held);
  vz::retain(&mut vm, held.clone())?;
  drop(held);
  owner.insert(id, vm)?;
  let runtime = tokio::runtime::Builder::new_multi_thread()
    .worker_threads(1)
    .enable_all()
    .build()?;
  let run_loop = NSRunLoop::currentRunLoop();
  let tick = |owner: &mut Owner| -> anyhow::Result<()> {
    owner.tick();
    ensure!(
      std::fs::metadata(&output)?.len() <= 1 << 20,
      "Guest diagnostic console exceeds bounds"
    );
    ensure!(
      !std::fs::read_to_string(&output)?.contains("HOPPER_COMMANDS_FAILED"),
      "Guest private-port setup failed"
    );
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
    Ok(())
  };
  fn wait<T: Send + 'static>(
    task: tokio::task::JoinHandle<anyhow::Result<T>>,
    owner: &mut Owner,
    tick: &impl Fn(&mut Owner) -> anyhow::Result<()>,
  ) -> anyhow::Result<T> {
    let deadline = Instant::now() + Duration::from_secs(195);
    while !task.is_finished() {
      if let Err(error) = tick(owner) {
        task.abort();
        return Err(error);
      }
      if Instant::now() >= deadline {
        task.abort();
        anyhow::bail!("Guest command probe timed out");
      }
    }
    futures_result(task)
  }
  fn futures_result<T>(mut task: tokio::task::JoinHandle<anyhow::Result<T>>) -> anyhow::Result<T> {
    use std::{
      future::Future,
      pin::Pin,
      task::{Context, Poll, Waker},
    };
    let mut context = Context::from_waker(Waker::noop());
    match Pin::new(&mut task).poll(&mut context) {
      Poll::Ready(result) => result?,
      Poll::Pending => anyhow::bail!("Completed task is not ready"),
    }
  }
  let start = client.clone();
  wait(
    runtime.spawn(async move { start.transition(id, Action::Start).await }),
    &mut owner,
    &tick,
  )?;
  let result = (|| -> anyhow::Result<()> {
    for (role, uid) in [(Role::Normal, 1001), (Role::Administrator, 0)] {
      let start = client.clone();
      let checks = (check.clone(), check.clone());
      let outcome = wait(
        runtime.spawn(async move {
          start
            .guest_request(
              id,
              1,
              0,
              Request::Start {
                command: Command {
                  executable: "id".into(),
                  args: vec!["-u".into()],
                  directory: None,
                  timeout_seconds: 10,
                  role,
                },
              },
              checks,
            )
            .await
        }),
        &mut owner,
        &tick,
      )?;
      let mut job = match outcome {
        Outcome::Job { job } => job,
        Outcome::Error { message } => anyhow::bail!("{message}"),
        Outcome::Transfer { .. } => {
          anyhow::bail!("Unexpected upload response to guest job request")
        }
      };
      let deadline = Instant::now() + Duration::from_secs(15);
      while !job.finished {
        ensure!(Instant::now() < deadline, "Guest identity job timed out");
        let poll = client.clone();
        let checks = (check.clone(), check.clone());
        let job_id = job.id.clone();
        job = match wait(
          runtime.spawn(async move {
            poll
              .guest_request(id, 1, 0, Request::Poll { job: job_id }, checks)
              .await
          }),
          &mut owner,
          &tick,
        )? {
          Outcome::Job { job } => job,
          Outcome::Error { message } => anyhow::bail!("{message}"),
          Outcome::Transfer { .. } => {
            anyhow::bail!("Unexpected upload response to guest job request")
          }
        };
      }
      fn identity(job: &Job, uid: u32) -> anyhow::Result<()> {
        ensure!(
          job.exit_code == Some(0)
            && job.stdout == format!("{uid}\n").as_bytes()
            && job
              .identity
              .as_ref()
              .is_some_and(|identity| identity.uid == uid),
          "Guest identity or output mismatch"
        );
        Ok(())
      }
      identity(&job, uid)?;
    }
    wait(
      runtime.spawn(files::check(client.clone(), id, check.clone())),
      &mut owner,
      &tick,
    )?;
    let start = client.clone();
    let checks = (check.clone(), check.clone());
    let outcome = wait(
      runtime.spawn(async move {
        start
          .guest_request(
            id,
            1,
            0,
            Request::Start {
              command: Command {
                executable: "sleep".into(),
                args: vec!["60".into()],
                directory: None,
                timeout_seconds: 120,
                role: Role::Normal,
              },
            },
            checks,
          )
          .await
      }),
      &mut owner,
      &tick,
    )?;
    let mut job = match outcome {
      Outcome::Job { job } => job,
      Outcome::Error { message } => anyhow::bail!("{message}"),
      Outcome::Transfer { .. } => anyhow::bail!("Unexpected upload response to guest job request"),
    };
    let cancel = client.clone();
    let checks = (check.clone(), check.clone());
    let job_id = job.id.clone();
    let cancelled = wait(
      runtime.spawn(async move {
        cancel
          .guest_request(id, 1, 0, Request::Cancel { job: job_id }, checks)
          .await
      }),
      &mut owner,
      &tick,
    )?;
    if let Outcome::Error { message } = cancelled {
      anyhow::bail!("{message}");
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while !job.finished {
      ensure!(
        Instant::now() < deadline,
        "Guest cancellation cleanup timed out"
      );
      let poll = client.clone();
      let checks = (check.clone(), check.clone());
      let job_id = job.id.clone();
      job = match wait(
        runtime.spawn(async move {
          poll
            .guest_request(id, 1, 0, Request::Poll { job: job_id }, checks)
            .await
        }),
        &mut owner,
        &tick,
      )? {
        Outcome::Job { job } => job,
        Outcome::Error { message } => anyhow::bail!("{message}"),
        Outcome::Transfer { .. } => {
          anyhow::bail!("Unexpected upload response to guest job request")
        }
      };
    }
    ensure!(
      job.phase == model::guest::Phase::Cancelled,
      "Guest cancellation was not confirmed"
    );
    ensure!(
      std::fs::read_to_string(&output)?.contains("HOPPER_COMMANDS_PRIVATE_OK"),
      "Guest private-access marker is missing"
    );
    Ok(())
  })();
  let stop = client.clone();
  let stopped = wait(
    runtime.spawn(async move { stop.transition(id, Action::Stop).await }),
    &mut owner,
    &|owner| {
      owner.tick();
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
      Ok(())
    },
  );
  result?;
  stopped?;
  owner.retire(id)?;
  let deadline = Instant::now() + Duration::from_secs(5);
  while weak.upgrade().is_some() && Instant::now() < deadline {
    run_loop.runMode_beforeDate(
      unsafe { NSDefaultRunLoopMode },
      &NSDate::dateWithTimeIntervalSinceNow(0.01),
    );
  }
  ensure!(
    weak.upgrade().is_none(),
    "Guest probe retained runtime ownership"
  );
  println!("Private boot delivery, normal/admin jobs, binary file transfers, upload cancellation/checksums, directory pages and runtime cleanup verified");
  Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Guest command probe requires Apple silicon macOS")
}
