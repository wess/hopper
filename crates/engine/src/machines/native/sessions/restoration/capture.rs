use super::Job;
use crate::machines::native::{
  self,
  sessions::{owner, snapshot::control},
  State,
};
use anyhow::{ensure, Context};
use machine::snapshot::{manifest::Binding, package::Description};
use model::native::{Command, Result as Reply};
use std::{
  sync::Arc,
  time::{SystemTime, UNIX_EPOCH},
};

pub(super) async fn prior(job: &Job) -> anyhow::Result<bool> {
  (job.check)()?;
  let running = matches!(native::state(&job.client), State::Running);
  if running {
    let paused = native::transact(&job.client, Command::Pause {}, Some(job.check.clone()))
      .await
      .and_then(|response| {
        ensure!(
          matches!(response.response.result, Reply::Paused {}),
          "Restore could not pause its source"
        );
        Ok(())
      });
    if let Err(error) = paused {
      resume(job, running).await;
      return Err(error);
    }
  }
  let outcome = async {
    let receipt = control::capture(&job.client, &job.rollback, job.tag, &job.check).await?;
    let hardware = job
      .client
      .hardware
      .as_ref()
      .context("Missing prior hardware")?;
    let parent = hardware.namespace(&job.manager, &job.id)?;
    let binding = Binding {
      machine: model::native::machine_identity(&job.id).context("Invalid prior VM identity")?,
      snapshot: model::native::machine_identity(&job.rollback).context("Invalid prior identity")?,
      configuration: job.client.configuration,
    };
    let layout = hardware.layout;
    let description = Description::new(
      "Before restore",
      SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?;
    let policy = job.policy.clone();
    let check = job.check.clone();
    tokio::task::spawn_blocking(move || {
      drop(policy.publish_checked(
        &parent,
        receipt,
        binding,
        layout,
        Some(&description),
        || check(),
      )?);
      check()
    })
    .await
    .context("Restore prior publication task failed")??;
    (job.check)()
  }
  .await;
  let cleanup = control::cleanup(&job.client, &job.rollback, job.tag).await;
  if let Err(error) = cleanup {
    let _ = native::request(&job.client, Command::Stop {}).await;
    if let Err(reap) = owner::finished(job.session.as_ref().context("Missing prior session")?).await
    {
      job
        .uncertain
        .store(true, std::sync::atomic::Ordering::Release);
      return Err(reap.context("Prior cleanup could not confirm source exit"));
    }
    return Err(error);
  }
  if outcome.is_err() {
    resume(job, running).await;
  }
  outcome.map(|()| running)
}

pub(super) async fn resume(job: &Job, running: bool) {
  if running && matches!(native::state(&job.client), State::Paused) {
    // cancellation may undo our pause only while the original policy remains authorized.
    let original = job.policy.clone();
    let check: native::Check = Arc::new(move || original.check());
    let _ = native::transact(&job.client, Command::Resume {}, Some(check)).await;
  }
}
