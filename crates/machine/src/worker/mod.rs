mod boot;
mod poll;
mod restore;
mod snapshot;
mod system;

use anyhow::{ensure, Context};
use machine::{ipc, runtime};
use model::native::{Command, Response, Result as Reply, StopReason};
use std::{
  fs::File,
  os::fd::{AsRawFd, FromRawFd},
  time::{Duration, Instant},
};

pub fn run() -> anyhow::Result<()> {
  let mut parent = poll::Parent {
    input: pipe(libc::STDIN_FILENO)?,
    output: ipc::Output::new(pipe(libc::STDOUT_FILENO)?)?,
    stream: ipc::Stream::default(),
    pending: None,
    held: false,
    stopping: None,
    input_since: None,
    setup: machine::setup::Decoder::default(),
    snapshots: None,
    restore: None,
  };
  let deadline = Instant::now() + Duration::from_secs(10);
  let request = loop {
    if let Some(request) = ipc::receive(&mut parent.stream, &mut parent.input)? {
      break request;
    }
    ensure!(
      Instant::now() < deadline,
      "Native parent did not provide boot configuration"
    );
    std::thread::sleep(Duration::from_millis(5));
  };
  let config = match request.command {
    Command::Start { boot } => boot,
    Command::StartRestored { boot, restore } => {
      return restore::run(parent, request.id, boot, restore)
    }
    Command::StartGeneration { boot, restore } => {
      return system::run(parent, request.id, boot, restore)
    }
    _ => {
      anyhow::bail!("Native worker requires boot configuration first");
    }
  };
  let machine_id = config.machine_id.clone();
  let store_path = config.store.clone();
  let boot::Prepared {
    boot,
    devices,
    mut store,
    configuration,
  } = boot::prepare(config)?;
  let captures = machine::hypervisor::state::group::create(boot.cpus as usize)?;
  parent.snapshots = Some(snapshot::Snapshots::new(
    &store_path,
    &machine_id,
    configuration,
    captures.clone(),
  )?);
  let mut initialized = false;
  let stopped =
    runtime::run_persistent_captured(boot, devices, &mut store, captures, |devices, ram, mode| {
      if !initialized {
        parent.reply(
          request.id,
          Reply::Started {
            machine_id: machine_id.clone(),
            configuration,
          },
          &[],
        )?;
        initialized = true;
      }
      parent.poll(devices, ram, mode)
    })?;
  ensure!(
    runtime::variables::bytes(&store) == stopped.variables,
    "Native firmware persistence differs from stopped guest"
  );
  let reason = match stopped.reason {
    runtime::Reason::Stopped => StopReason::Requested,
    runtime::Reason::Shutdown => StopReason::Shutdown,
    runtime::Reason::Reset => StopReason::Reset,
  };
  // flush an earlier reply before announcing shutdown; id zero denotes a guest power event.
  drain(&mut parent)?;
  parent.output.send(
    &Response {
      id: parent.stopping.unwrap_or(0),
      result: Reply::Stopped { reason },
    },
    &[],
  )?;
  drain(&mut parent)?;
  Ok(())
}

fn drain(parent: &mut poll::Parent) -> anyhow::Result<()> {
  let deadline = Instant::now() + Duration::from_secs(5);
  while parent.output.pending()? {
    ensure!(
      Instant::now() < deadline,
      "Native parent stopped receiving replies"
    );
    std::thread::sleep(Duration::from_millis(1));
  }
  Ok(())
}

fn pipe(fd: i32) -> anyhow::Result<File> {
  let duplicated = unsafe { libc::dup(fd) };
  ensure!(duplicated >= 0, "Duplicate native parent pipe");
  let file = unsafe { File::from_raw_fd(duplicated) };
  let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
  ensure!(flags >= 0, "Read native pipe flags");
  if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
    return Err(std::io::Error::last_os_error()).context("Set native parent pipe nonblocking");
  }
  Ok(file)
}
