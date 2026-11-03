use anyhow::{ensure, Context};
use machine::vz::queue::{Check, Client};
use model::guest::{
  files::{Create, Read, Receipt, CREATE_LIMIT},
  wire::{Action, Outcome},
  Job, Role,
};
use std::time::{Duration, Instant};
#[path = "files/browse.rs"]
mod browse;
#[path = "files/upload.rs"]
mod upload;

async fn run(client: &Client, id: &str, check: &Check, action: Action) -> anyhow::Result<Job> {
  let checks = || (check.clone(), check.clone());
  let job = match client.guest_request(id, 1, 0, action, checks()).await? {
    Outcome::Job { job } => job,
    Outcome::Error { message } => anyhow::bail!("{message}"),
    Outcome::Transfer { .. } => anyhow::bail!("Unexpected upload response to guest job request"),
  };
  finish(client, id, check, job).await
}

async fn finish(client: &Client, id: &str, check: &Check, mut job: Job) -> anyhow::Result<Job> {
  let checks = || (check.clone(), check.clone());
  let deadline = Instant::now() + Duration::from_secs(40);
  while !job.finished {
    ensure!(Instant::now() < deadline, "Guest file job did not finish");
    tokio::time::sleep(Duration::from_millis(20)).await;
    job = match client
      .guest_request(id, 1, 0, Action::Poll { job: job.id }, checks())
      .await?
    {
      Outcome::Job { job } => job,
      Outcome::Error { message } => anyhow::bail!("{message}"),
      Outcome::Transfer { .. } => anyhow::bail!("Unexpected upload response to guest job request"),
    };
  }
  ensure!(
    !job.stdout_truncated && !job.stderr_truncated && job.cleanup_error.is_none(),
    "Guest file job output or cleanup was incomplete"
  );
  Ok(job)
}

pub async fn check(client: Client, id: &'static str, check: Check) -> anyhow::Result<()> {
  let bytes = (0..CREATE_LIMIT)
    .map(|index| index as u8)
    .collect::<Vec<_>>();
  for (role, uid, name) in [
    (Role::Normal, 1001, "normal"),
    (Role::Administrator, 0, "admin"),
  ] {
    let path = format!("/home/hopper/{name}.bin");
    let file = Create {
      path: path.clone(),
      bytes: bytes.clone(),
      role,
    };
    let created = run(
      &client,
      id,
      &check,
      Action::CreateFile { file: file.clone() },
    )
    .await?;
    ensure!(
      created.exit_code == Some(0)
        && created
          .identity
          .as_ref()
          .is_some_and(|identity| identity.uid == uid),
      "Guest file creation identity or exit mismatch"
    );
    let receipt: Receipt = serde_json::from_slice(&created.stdout)?;
    ensure!(
      receipt.length == bytes.len() && receipt.eof && receipt.stamp.valid(),
      "Guest file creation receipt mismatch"
    );
    let mut read = Read {
      path: path.clone(),
      offset: 0,
      length: CREATE_LIMIT,
      stamp: Some(receipt.stamp),
      role,
    };
    let received = run(
      &client,
      id,
      &check,
      Action::Start {
        command: read.command().context("Build guest file read")?,
      },
    )
    .await?;
    ensure!(
      received.exit_code == Some(0) && received.stdout == bytes,
      "Guest uploaded bytes did not round-trip"
    );
    let read_receipt: Receipt = serde_json::from_slice(&received.stderr)?;
    ensure!(
      read_receipt.stamp == receipt.stamp && read_receipt.eof,
      "Guest file stamp or EOF changed"
    );
    let mut replacement = file;
    replacement.bytes = b"replacement".to_vec();
    let existing = run(
      &client,
      id,
      &check,
      Action::CreateFile { file: replacement },
    )
    .await?;
    ensure!(
      existing.exit_code.is_some_and(|code| code != 0),
      "Existing guest destination was replaced"
    );
    let preserved = run(
      &client,
      id,
      &check,
      Action::Start {
        command: read.command().unwrap(),
      },
    )
    .await?;
    ensure!(
      preserved.exit_code == Some(0) && preserved.stdout == bytes,
      "Existing guest file content or identity changed after rejected creation"
    );
    if role == Role::Administrator {
      read.role = Role::Normal;
      let denied = run(
        &client,
        id,
        &check,
        Action::Start {
          command: read.command().unwrap(),
        },
      )
      .await?;
      ensure!(
        denied.exit_code.is_some_and(|code| code != 0) && denied.stdout.is_empty(),
        "Normal guest file read exposed administrator-owned payload"
      );
    }
  }
  upload::check(&client, id, &check).await?;
  browse::check(&client, id, &check).await
}
