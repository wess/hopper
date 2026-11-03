pub mod auth;
mod framing;

use super::{Command, Job, Phase};
use crate::GuestOs;
use serde::{Deserialize, Serialize};
use std::io;

pub use framing::Decoder;
pub const CHUNK: usize = 64 << 10;

#[derive(Clone, Copy, Debug)]
pub enum Direction {
  Request,
  Reply,
}

impl Direction {
  pub(super) fn limit(self) -> usize {
    match self {
      Self::Request => 256 << 10,
      Self::Reply => 1 << 20,
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
  Start {
    command: Command,
  },
  CreateFile {
    file: super::files::Create,
  },
  BeginUpload {
    file: super::files::Upload,
  },
  UploadChunk {
    job: String,
    offset: u64,
    bytes: Vec<u8>,
  },
  UploadStatus {
    job: String,
  },
  CommitUpload {
    job: String,
  },
  Poll {
    job: String,
  },
  Cancel {
    job: String,
  },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
  pub sequence: u64,
  pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase", deny_unknown_fields)]
pub enum Outcome {
  Job { job: Job },
  Transfer { transfer: super::files::Transfer },
  Error { message: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reply {
  pub sequence: u64,
  pub outcome: Outcome,
}

#[derive(Clone, Debug)]
pub enum Message {
  Request(Request),
  Reply(Reply),
}

fn identifier(value: &str) -> bool {
  value.len() == 36
    && uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value && !id.is_nil())
}

fn text(value: &str, limit: usize) -> bool {
  value.len() <= limit && !value.contains('\0')
}

impl Request {
  pub fn valid_for(&self, guest: GuestOs) -> bool {
    self.sequence != 0
      && match &self.action {
        Action::Start { command } => command.valid_for(guest),
        Action::CreateFile { file } => guest == GuestOs::Linux && file.command().is_some(),
        Action::BeginUpload { file } => guest == GuestOs::Linux && file.command().is_some(),
        Action::UploadChunk { job, offset, bytes } => {
          guest == GuestOs::Linux
            && identifier(job)
            && !bytes.is_empty()
            && bytes.len() <= super::files::CREATE_LIMIT
            && offset
              .checked_add(bytes.len() as u64)
              .is_some_and(|end| end <= super::files::UPLOAD_LIMIT)
        }
        Action::UploadStatus { job } | Action::CommitUpload { job } => {
          guest == GuestOs::Linux && identifier(job)
        }
        Action::Poll { job } | Action::Cancel { job } => identifier(job),
      }
  }

  pub fn encode(&self, guest: GuestOs) -> io::Result<Vec<u8>> {
    require(self.valid_for(guest))?;
    framing::encode(self, Direction::Request)
  }
}

impl Reply {
  pub fn valid(&self) -> bool {
    self.sequence != 0
      && match &self.outcome {
        Outcome::Error { message } => !message.is_empty() && text(message, 512),
        Outcome::Transfer { transfer } => {
          transfer.size <= super::files::UPLOAD_LIMIT
            && transfer.offset <= transfer.size
            && (!transfer.committed || transfer.offset == transfer.size)
            && valid_job(&transfer.job)
        }
        Outcome::Job { job } => valid_job(job),
      }
  }

  pub fn encode(&self) -> io::Result<Vec<u8>> {
    require(self.valid())?;
    framing::encode(self, Direction::Reply)
  }
}

fn valid_job(job: &Job) -> bool {
  identifier(&job.id)
    && match job.phase {
      Phase::Starting => !job.finished && job.identity.is_none(),
      Phase::Running => !job.finished && job.identity.is_some(),
      Phase::Completed => job.finished && job.identity.is_some(),
      Phase::Failed | Phase::Cancelled | Phase::TimedOut => job.finished,
      Phase::CleanupFailed => true,
    }
    && (job.finished || (job.exit_code.is_none() && job.signal.is_none()))
    && job.stdout.len() <= 64 << 10
    && job.stderr.len() <= 64 << 10
    && job.error.as_ref().is_none_or(|error| text(error, 512))
    && job
      .cleanup_error
      .as_ref()
      .is_none_or(|error| text(error, 512))
    && job.identity.as_ref().is_none_or(|identity| {
      !identity.user.is_empty()
        && text(&identity.user, 256)
        && !identity.groups.is_empty()
        && identity.groups.len() <= 64
        && identity.groups.contains(&identity.gid)
        && identity.administrator == (identity.uid == 0)
    })
}

fn require(valid: bool) -> io::Result<()> {
  if valid {
    Ok(())
  } else {
    Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Invalid guest job message",
    ))
  }
}
