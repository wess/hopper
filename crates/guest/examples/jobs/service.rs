use anyhow::ensure;
use model::{
  guest::{
    wire::{
      auth::{Boot, GuestHandshake, HostHandshake, Scope, Secret, Session},
      Action, Message, Outcome, Reply, Request,
    },
    Command, Job, Role,
  },
  GuestOs,
};
use std::{
  path::Path,
  time::{Duration, Instant},
};

fn exchange(
  host: &mut Session,
  service: &mut guest::service::Service,
  sequence: &mut u64,
  action: Action,
) -> anyhow::Result<Job> {
  let request = host.send(&Message::Request(Request {
    sequence: *sequence,
    action,
  }))?;
  let mut response = None;
  for chunk in request.chunks(3) {
    if let Some(reply) = service.feed(chunk)? {
      ensure!(response.is_none(), "Guest service replied twice");
      response = Some(reply);
    }
  }
  let Message::Reply(Reply { outcome, .. }) = host.receive(&response.unwrap())? else {
    anyhow::bail!("Authenticated guest service returned the wrong direction");
  };
  *sequence += 1;
  match outcome {
    Outcome::Job { job } => Ok(job),
    Outcome::Error { message } => anyhow::bail!("{message}"),
    Outcome::Transfer { .. } => anyhow::bail!("Unexpected upload response to guest job request"),
  }
}

pub(super) fn run() -> anyhow::Result<()> {
  let root = Path::new("/sys/fs/cgroup/hopper");
  let scope = Scope {
    boot: Boot {
      machine: model::new_uuid(),
      generation: 1,
      guest: GuestOs::Linux,
    },
    policy: 0,
  };
  let host = HostHandshake::begin(Secret::from_bytes([11; 32])?, scope.clone())?;
  let (guest, answer) =
    GuestHandshake::answer(Secret::from_bytes([11; 32])?, &scope.boot, &host.hello())?;
  let (mut host, confirmation) = host.finish(&answer)?;
  let mut service = guest::service::open(guest.finish(&confirmation)?, root)?;
  let mut sequence = 1;
  for role in [Role::Normal, Role::Administrator] {
    let mut job = exchange(
      &mut host,
      &mut service,
      &mut sequence,
      Action::Start {
        command: Command {
          executable: "/usr/bin/id".into(),
          args: vec!["-u".into()],
          directory: None,
          role,
          timeout_seconds: 5,
        },
      },
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !job.finished {
      ensure!(
        Instant::now() < deadline,
        "Authenticated job did not finish"
      );
      std::thread::sleep(Duration::from_millis(10));
      job = exchange(
        &mut host,
        &mut service,
        &mut sequence,
        Action::Poll { job: job.id },
      )?;
    }
    let uid = if role == Role::Normal { 1001 } else { 0 };
    ensure!(
      job.identity.is_some_and(|identity| identity.uid == uid)
        && job.stdout == format!("{uid}\n").as_bytes(),
      "Authenticated job used the wrong identity"
    );
  }
  let sleeping = exchange(
    &mut host,
    &mut service,
    &mut sequence,
    Action::Start {
      command: Command {
        executable: "/bin/sh".into(),
        args: vec![
          "-c".into(),
          "echo $$ >/tmp/hopperauthpid; exec sleep 60".into(),
        ],
        directory: None,
        role: Role::Normal,
        timeout_seconds: 60,
      },
    },
  )?;
  let deadline = Instant::now() + Duration::from_secs(5);
  while !Path::new("/tmp/hopperauthpid").exists() {
    ensure!(
      Instant::now() < deadline,
      "Authenticated child did not start"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  let pid = std::fs::read_to_string("/tmp/hopperauthpid")?
    .trim()
    .parse::<u32>()?;
  let mut forged = host.send(&Message::Request(Request {
    sequence,
    action: Action::Poll { job: sleeping.id },
  }))?;
  let end = forged.len() - 1;
  forged[end] ^= 1;
  ensure!(
    service.feed(&forged).is_err(),
    "Forged guest request was admitted"
  );
  ensure!(
    service.feed(&forged).is_err(),
    "Closed guest service reopened"
  );
  while !service.drained() {
    ensure!(
      Instant::now() < deadline,
      "Closed guest service did not drain"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  drop(service);
  loop {
    if let Ok(reopened) = guest::jobs::open(root) {
      ensure!(reopened.idle(), "Disconnected service retained active jobs");
      break;
    }
    ensure!(
      Instant::now() < deadline,
      "Guest authentication failure did not cancel its jobs"
    );
    std::thread::sleep(Duration::from_millis(10));
  }
  if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
    ensure!(
      status
        .lines()
        .any(|line| line.starts_with("State:") && line.contains("Z (zombie)")),
      "Disconnected guest service left its process running"
    );
  }
  Ok(())
}
