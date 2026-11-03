use crate::jobs::{self, Jobs};
use anyhow::ensure;
use model::{
  guest::wire::{auth::Session, Action, Message, Outcome, Reply},
  GuestOs,
};
use std::path::Path;

pub struct Service {
  session: Session,
  jobs: Jobs,
}

pub fn open(session: Session, root: &Path) -> anyhow::Result<Service> {
  ensure!(
    !session.is_host() && session.scope().boot.guest == GuestOs::Linux,
    "Guest service requires an authenticated Linux guest connection"
  );
  Ok(Service {
    session,
    jobs: jobs::open(root)?,
  })
}

impl Service {
  pub fn feed(&mut self, bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    let result = self.dispatch(bytes);
    if result.is_err() {
      self.close();
    }
    result
  }

  fn dispatch(&mut self, bytes: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    let Some(message) = self.session.feed(bytes)? else {
      return Ok(None);
    };
    let Message::Request(request) = message else {
      anyhow::bail!("Guest service received a reply");
    };
    let jobs = &mut self.jobs;
    let result = match request.action {
      Action::Start { command } => jobs.start(command).map(|job| Outcome::Job { job }),
      Action::CreateFile { file } => jobs.create_file(file).map(|job| Outcome::Job { job }),
      Action::Poll { job } => jobs.poll(&job, false).map(|job| Outcome::Job { job }),
      Action::Cancel { job } => jobs.poll(&job, true).map(|job| Outcome::Job { job }),
      Action::BeginUpload { file } => jobs
        .begin_upload(file)
        .map(|transfer| Outcome::Transfer { transfer }),
      Action::UploadChunk { job, offset, bytes } => jobs
        .upload_chunk(&job, offset, &bytes)
        .map(|transfer| Outcome::Transfer { transfer }),
      Action::UploadStatus { job } => jobs
        .upload_status(&job)
        .map(|transfer| Outcome::Transfer { transfer }),
      Action::CommitUpload { job } => jobs
        .commit_upload(&job)
        .map(|transfer| Outcome::Transfer { transfer }),
    };
    let outcome = match result {
      Ok(outcome) => outcome,
      Err(error) => Outcome::Error {
        message: jobs::error(format!("{error:#}")),
      },
    };
    let reply = self.session.send(&Message::Reply(Reply {
      sequence: request.sequence,
      outcome,
    }))?;
    Ok(Some(reply))
  }

  pub fn close(&mut self) {
    self.session.close();
    self.jobs.cancel_all();
  }

  pub fn drained(&self) -> bool {
    self.jobs.idle()
  }
}

impl Drop for Service {
  fn drop(&mut self) {
    self.close();
  }
}
