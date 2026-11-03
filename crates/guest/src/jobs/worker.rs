use super::{group::Group, output, Control};
use anyhow::Context;
use model::guest::{Command, Identity, Phase};
use std::{
  ffi::CString,
  os::{
    fd::AsRawFd,
    unix::process::{CommandExt, ExitStatusExt},
  },
  process::{Child, ChildStderr, ChildStdout, Stdio},
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
  },
  time::{Duration, Instant},
};

pub(super) fn spawn(
  command: &Command,
  identity: &Identity,
  home: &str,
  group: &Group,
  control: &Arc<Control>,
  input: Option<std::fs::File>,
) -> anyhow::Result<(Child, ChildStdout, ChildStderr)> {
  let mut process = std::process::Command::new(&command.executable);
  process
    .args(&command.args)
    .env_clear()
    .env("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
    .env("HOME", home)
    .env("USER", &identity.user)
    .env("LOGNAME", &identity.user)
    .stdin(input.map(Stdio::from).unwrap_or_else(Stdio::null))
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
  let (fd, uid, gid, groups) = (
    group.admission.as_raw_fd(),
    identity.uid,
    identity.gid,
    identity.groups.clone(),
  );
  let control = control.clone();
  let directory = CString::new(command.directory.as_deref().unwrap_or(home))?;
  unsafe {
    process.pre_exec(move || {
      // attach before privilege reduction; only async-signal-safe calls follow fork.
      if control.cancel.load(Ordering::Acquire) {
        return Err(std::io::Error::from_raw_os_error(libc::ECANCELED));
      }
      if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
        || libc::prctl(libc::PR_SET_KEEPCAPS, 0, 0, 0, 0) != 0
        || libc::write(fd, b"0\n".as_ptr().cast(), 2) != 2
        || libc::setgroups(groups.len(), groups.as_ptr()) != 0
        || libc::setresgid(gid, gid, gid) != 0
        || libc::setresuid(uid, uid, uid) != 0
        || libc::getuid() != uid
        || libc::getgid() != gid
        || libc::chdir(directory.as_ptr()) != 0
      {
        return Err(std::io::Error::last_os_error());
      }
      Ok(())
    });
  }
  let mut child = process.spawn().context("Start guest command")?;
  let stdout = child.stdout.take().context("Guest stdout is absent")?;
  let stderr = child.stderr.take().context("Guest stderr is absent")?;
  for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
      let _ = group.kill();
      let _ = child.wait();
      anyhow::bail!(
        "Configure guest output pipe: {}",
        std::io::Error::last_os_error()
      );
    }
  }
  Ok((child, stdout, stderr))
}

pub(super) fn run(
  job: Arc<Control>,
  command: Command,
  identity: Identity,
  home: String,
  group: Group,
  accepted: Instant,
  input: Option<std::fs::File>,
) {
  let group = Arc::new(group);
  let (send, receive) = std::sync::mpsc::sync_channel(1);
  let launch_group = group.clone();
  let launch_job = job.clone();
  let deadline = accepted + Duration::from_secs(command.timeout_seconds.into());
  let actual = identity.clone();
  let launch = std::thread::Builder::new()
    .name("hopperlaunch".into())
    .spawn(move || {
      let result = spawn(
        &command,
        &identity,
        &home,
        &launch_group,
        &launch_job,
        input,
      );
      let _ = send.send(result);
    });
  let stop = Arc::new(AtomicBool::new(false));
  let mut readers = Vec::new();
  let mut child = None;
  let mut launched = launch.is_err();
  let mut finish = launch.as_ref().err().map(|error| {
    job.result.lock().unwrap().error = Some(super::error(error));
    (Phase::Failed, Instant::now())
  });
  let mut status = None;
  let mut cleanup = None;
  loop {
    if finish.is_none() && job.cancel.load(Ordering::Acquire) {
      finish = Some((Phase::Cancelled, Instant::now()));
    }
    if finish.is_none() && Instant::now() >= deadline {
      job.cancel.store(true, Ordering::Release);
      finish = Some((Phase::TimedOut, Instant::now()));
    }
    if !launched {
      match receive.try_recv() {
        Ok(Ok((process, stdout, stderr))) => {
          launched = true;
          let mut result = job.result.lock().unwrap();
          result.identity = Some(actual.clone());
          result.phase = Phase::Running;
          drop(result);
          for reader in [
            output::drain(job.clone(), stdout, false, stop.clone()),
            output::drain(job.clone(), stderr, true, stop.clone()),
          ] {
            match reader {
              Ok(reader) => readers.push(reader),
              Err(error) => {
                job.result.lock().unwrap().error = Some(super::error(error));
                finish.get_or_insert((Phase::Failed, Instant::now()));
              }
            }
          }
          child = Some(process);
        }
        Ok(Err(error)) => {
          launched = true;
          job.result.lock().unwrap().error = Some(super::error(format!("{error:#}")));
          finish.get_or_insert((Phase::Failed, Instant::now()));
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
          launched = true;
          finish.get_or_insert((Phase::Failed, Instant::now()));
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
      }
    }
    if let Some(child) = &mut child {
      match child.try_wait() {
        Ok(Some(exit)) => {
          status = Some(exit);
          finish.get_or_insert((Phase::Completed, Instant::now()));
        }
        Ok(None) => {}
        Err(error) => {
          cleanup = Some(super::error(error));
          finish.get_or_insert((Phase::CleanupFailed, Instant::now()));
        }
      }
    }
    if let Some((phase, began)) = finish {
      if let Err(error) = group.kill() {
        cleanup = Some(super::error(error));
      }
      match group.empty() {
        Ok(true) if launched && (child.is_none() || status.is_some()) => {
          if let Err(error) = group.retire() {
            cleanup = Some(super::error(&error));
            if began.elapsed() >= Duration::from_secs(5) {
              let mut result = job.result.lock().unwrap();
              result.phase = Phase::CleanupFailed;
              result.cleanup_error = Some(super::error(error));
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
          }
          stop.store(true, Ordering::Release);
          for reader in readers {
            let _ = reader.join();
          }
          if let Ok(launch) = launch {
            let _ = launch.join();
          }
          let mut result = job.result.lock().unwrap();
          result.phase = if phase == Phase::Completed && result.error.is_some() {
            Phase::Failed
          } else {
            phase
          };
          result.finished = true;
          result.exit_code = status.and_then(|exit| exit.code());
          result.signal = status.and_then(|exit| exit.signal());
          if cleanup.is_some() {
            result.cleanup_error = cleanup;
          }
          drop(result);
          drop(group);
          job.done.store(true, Ordering::Release);
          return;
        }
        Err(error) => {
          cleanup = Some(super::error(error));
        }
        _ => {}
      }
      if began.elapsed() >= Duration::from_secs(5) {
        let mut result = job.result.lock().unwrap();
        result.phase = Phase::CleanupFailed;
        result.cleanup_error = Some(
          cleanup
            .clone()
            .unwrap_or_else(|| "Guest process tree has not exited".into()),
        );
      }
    }
    std::thread::sleep(Duration::from_millis(
      if finish.is_some_and(|(_, began)| began.elapsed() >= Duration::from_secs(5)) {
        1000
      } else {
        10
      },
    ));
  }
}
