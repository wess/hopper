#[cfg(target_os = "linux")]
#[path = "jobs/service.rs"]
mod service;

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
  use anyhow::ensure;
  use model::guest::{
    wire::{Decoder, Direction, Message, Outcome, Reply, CHUNK},
    Command, Job, Phase, Role,
  };
  use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
  };

  if std::env::args().nth(1).as_deref() == Some("--identity") {
    println!("{} {}", unsafe { libc::getuid() }, unsafe {
      libc::geteuid()
    });
    return Ok(());
  }
  let mut jobs = guest::jobs::open(Path::new("/sys/fs/cgroup/hopper"))?;
  ensure!(
    guest::jobs::open(Path::new("/sys/fs/cgroup/hopper")).is_err(),
    "Duplicate job supervision was admitted"
  );
  let command = |executable: &str, args: &[&str], role, timeout_seconds| Command {
    executable: executable.into(),
    args: args.iter().map(|arg| (*arg).into()).collect(),
    directory: None,
    timeout_seconds,
    role,
  };
  let wait = |jobs: &guest::jobs::Jobs, id: &str, cancel| -> anyhow::Result<Job> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      let job = jobs.poll(id, cancel)?;
      let encoded = Reply {
        sequence: 1,
        outcome: Outcome::Job { job },
      }
      .encode()?;
      let mut decoder = Decoder::new(Direction::Reply, model::GuestOs::Linux);
      for chunk in encoded.chunks(CHUNK) {
        decoder.feed(chunk)?;
      }
      let Some(Message::Reply(Reply {
        outcome: Outcome::Job { job },
        ..
      })) = decoder.take()?
      else {
        anyhow::bail!("Guest job reply did not survive framing");
      };
      if job.finished {
        return Ok(job);
      }
      ensure!(
        Instant::now() < deadline,
        "Guest job timed out: {:?}",
        job.phase
      );
      std::thread::sleep(Duration::from_millis(10));
    }
  };
  let normal = jobs.start(command("/usr/bin/id", &["-u"], Role::Normal, 5))?;
  let normal = wait(&jobs, &normal.id, false)?;
  ensure!(
    normal.phase == Phase::Completed && normal.stdout == b"1001\n",
    "Normal command did not run as UID 1001: {normal:?}"
  );
  let identity = normal.identity.unwrap();
  ensure!(
    identity.uid == 1001 && !identity.administrator,
    "Normal identity is incorrect"
  );
  std::fs::copy(std::env::current_exe()?, "/tmp/hopperpriv")?;
  std::fs::set_permissions("/tmp/hopperpriv", std::fs::Permissions::from_mode(0o4755))?;
  let privilege = jobs.start(command("/tmp/hopperpriv", &["--identity"], Role::Normal, 5))?;
  let privilege = wait(&jobs, &privilege.id, false)?;
  ensure!(
    privilege.stdout == b"1001 1001\n",
    "Normal execution acquired setuid privileges"
  );
  let named = jobs.start(command("id", &["-u"], Role::Normal, 5))?;
  ensure!(
    wait(&jobs, &named.id, false)?.stdout == b"1001\n",
    "Guest executable search used the wrong environment"
  );
  let capabilities = jobs.start(command(
    "/bin/sh",
    &["-c", "grep CapEff /proc/self/status"],
    Role::Normal,
    5,
  ))?;
  let capabilities = wait(&jobs, &capabilities.id, false)?;
  ensure!(
    String::from_utf8(capabilities.stdout)?
      .split_whitespace()
      .last()
      == Some("0000000000000000"),
    "Normal guest command retained capabilities"
  );
  let groups = jobs.start(command("/usr/bin/id", &["-G"], Role::Normal, 5))?;
  let groups = wait(&jobs, &groups.id, false)?;
  let mut reported = groups.identity.unwrap().groups;
  let mut actual = String::from_utf8(groups.stdout)?
    .split_whitespace()
    .map(str::parse::<u32>)
    .collect::<Result<Vec<_>, _>>()?;
  reported.sort_unstable();
  reported.dedup();
  actual.sort_unstable();
  actual.dedup();
  ensure!(
    reported == actual,
    "Guest supplementary groups do not match execution"
  );
  let admin = jobs.start(command("/usr/bin/id", &["-u"], Role::Administrator, 5))?;
  let admin = wait(&jobs, &admin.id, false)?;
  ensure!(
    admin.phase == Phase::Completed
      && admin.stdout == b"0\n"
      && admin
        .identity
        .as_ref()
        .is_some_and(|identity| identity.uid == 0 && identity.administrator),
    "Administrator identity is incorrect: {admin:?}"
  );
  let literal = jobs.start(command("/bin/echo", &["literal; id"], Role::Normal, 5))?;
  let literal = wait(&jobs, &literal.id, false)?;
  ensure!(
    literal.stdout == b"literal; id\n",
    "Command arguments became a shell script"
  );
  let nested = jobs.start(command("/bin/sh", &["-c", "group=$(sed -n 's/^0:://p' /proc/self/cgroup); mkdir /sys/fs/cgroup$group/nested; echo nested"], Role::Administrator, 5))?;
  let nested = wait(&jobs, &nested.id, false)?;
  ensure!(
    nested.phase == Phase::Completed && nested.stdout == b"nested\n",
    "Nested job cgroup cleanup failed: {:?}",
    nested.phase
  );
  let missing = jobs.start(command("/does/not/exist", &[], Role::Normal, 5))?;
  let missing = wait(&jobs, &missing.id, false)?;
  ensure!(
    missing.phase == Phase::Failed && missing.identity.is_none() && missing.error.is_some(),
    "Failed startup fabricated an execution identity"
  );
  let unicode = jobs.start(command(
    &format!("/{}", "界".repeat(1000)),
    &[],
    Role::Normal,
    5,
  ))?;
  let unicode = wait(&jobs, &unicode.id, false)?;
  ensure!(
    unicode.phase == Phase::Failed
      && unicode.identity.is_none()
      && unicode
        .error
        .as_ref()
        .is_some_and(|error| error.len() <= 512),
    "Unicode launch error exceeded the wire limit"
  );
  let mut private = command("/usr/bin/id", &["-u"], Role::Normal, 5);
  private.directory = Some("/root".into());
  let private = jobs.start(private)?;
  let private = wait(&jobs, &private.id, false)?;
  ensure!(
    private.phase == Phase::Failed && private.identity.is_none(),
    "Normal execution entered a private root directory"
  );
  let pending = jobs.start(command("/bin/sleep", &["60"], Role::Normal, 5))?;
  let pending = wait(&jobs, &pending.id, true)?;
  ensure!(
    pending.phase == Phase::Cancelled,
    "Cancellation during startup became an execution failure"
  );
  let flood = jobs.start(command(
    "/bin/sh",
    &["-c", "yes x | head -c 131072; yes e | head -c 131072 >&2"],
    Role::Normal,
    5,
  ))?;
  let flood = wait(&jobs, &flood.id, false)?;
  ensure!(
    flood.phase == Phase::Completed
      && flood.stdout.len() == 64 << 10
      && flood.stderr.len() == 64 << 10
      && flood.stdout_truncated
      && flood.stderr_truncated,
    "Output did not drain within its limits: {flood:?}"
  );
  let tree = jobs.start(command(
    "/bin/sh",
    &[
      "-c",
      "setsid sh -c 'echo $$ > /tmp/hopperescaped; sleep 60' & wait",
    ],
    Role::Normal,
    5,
  ))?;
  let timed = jobs.start(command("/bin/sleep", &["60"], Role::Normal, 1))?;
  ensure!(
    jobs
      .start(command("/bin/echo", &["third"], Role::Normal, 5))
      .is_err(),
    "Third concurrent job was admitted"
  );
  let deadline = Instant::now() + Duration::from_secs(3);
  while !Path::new("/tmp/hopperescaped").exists() {
    ensure!(
      Instant::now() < deadline,
      "Detached descendant did not start"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  let pid = std::fs::read_to_string("/tmp/hopperescaped")?
    .trim()
    .parse::<u32>()?;
  let tree = wait(&jobs, &tree.id, true)?;
  ensure!(
    tree.phase == Phase::Cancelled && tree.signal == Some(9),
    "Process tree was not cancelled: {tree:?}"
  );
  if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
    ensure!(
      status
        .lines()
        .any(|line| line.starts_with("State:") && line.contains("Z (zombie)")),
      "Detached descendant is still alive"
    );
  }
  let timed = wait(&jobs, &timed.id, false)?;
  ensure!(
    timed.phase == Phase::TimedOut && timed.signal == Some(9),
    "Timeout did not cancel the guest tree"
  );
  let after = jobs.start(command("/bin/echo", &["after"], Role::Normal, 5))?;
  ensure!(
    wait(&jobs, &after.id, false)?.stdout == b"after\n",
    "Capacity did not recover after cleanup"
  );
  ensure!(jobs.idle(), "Guest jobs retained active ownership");
  for _ in 0..18 {
    let history = jobs.start(command("/bin/echo", &["history"], Role::Normal, 5))?;
    ensure!(
      wait(&jobs, &history.id, false)?.finished,
      "History job did not finish"
    );
  }
  ensure!(
    jobs.poll(&normal.id, false).is_err(),
    "Completed job history grew without eviction"
  );
  let abandoned = jobs.start(command("/bin/sleep", &["60"], Role::Normal, 5))?;
  let deadline = Instant::now() + Duration::from_secs(5);
  while jobs.poll(&abandoned.id, false)?.phase == Phase::Starting {
    ensure!(Instant::now() < deadline, "Abandoned job did not start");
    std::thread::sleep(Duration::from_millis(10));
  }
  drop(jobs);
  loop {
    if let Ok(reopened) = guest::jobs::open(Path::new("/sys/fs/cgroup/hopper")) {
      ensure!(reopened.idle(), "Reopened service recovered phantom jobs");
      break;
    }
    ensure!(
      Instant::now() < deadline,
      "Dropped job service did not clean up"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  std::fs::create_dir("/sys/fs/cgroup/hopper/recovery")?;
  ensure!(
    guest::jobs::open(Path::new("/sys/fs/cgroup/hopper")).is_err(),
    "Unknown previous job group was silently ignored"
  );
  ensure!(
    Path::new("/sys/fs/cgroup/hopper/recovery").exists(),
    "Unknown job group was deleted"
  );
  std::fs::remove_dir("/sys/fs/cgroup/hopper/recovery")?;
  service::run()?;
  println!("HOPPER_JOBS_OK");
  Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() -> anyhow::Result<()> {
  anyhow::bail!("Guest jobs diagnostic requires Linux")
}
