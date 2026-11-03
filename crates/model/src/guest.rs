use crate::GuestOs;
use serde::{Deserialize, Serialize};

pub mod files;
pub mod wire;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
  Normal,
  Administrator,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Command {
  pub executable: String,
  pub args: Vec<String>,
  pub directory: Option<String>,
  pub timeout_seconds: u16,
  pub role: Role,
}

impl Command {
  pub fn valid_for(&self, guest: GuestOs) -> bool {
    let bounded = |value: &str| !value.contains('\0') && value.len() <= 4096;
    let absolute = |value: &str| match guest {
      GuestOs::Linux | GuestOs::Macos => value.starts_with('/'),
      GuestOs::Windows => {
        let bytes = value.as_bytes();
        bytes.len() >= 3
          && bytes[0].is_ascii_alphabetic()
          && bytes[1] == b':'
          && matches!(bytes[2], b'\\' | b'/')
      }
    };
    let name = !self.executable.is_empty()
      && !self
        .executable
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '/' | '\\' | ':'));
    (absolute(&self.executable) || name)
      && bounded(&self.executable)
      && self.args.len() <= 128
      && self.args.iter().all(|arg| bounded(arg))
      && self.args.iter().map(|arg| arg.len()).sum::<usize>() <= 32 << 10
      && self
        .directory
        .as_ref()
        .is_none_or(|path| absolute(path) && bounded(path))
      && (1..=600).contains(&self.timeout_seconds)
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Identity {
  pub user: String,
  pub uid: u32,
  pub gid: u32,
  pub groups: Vec<u32>,
  pub administrator: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
  Starting,
  Running,
  Failed,
  Completed,
  Cancelled,
  TimedOut,
  CleanupFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Job {
  pub id: String,
  pub identity: Option<Identity>,
  pub phase: Phase,
  pub finished: bool,
  pub exit_code: Option<i32>,
  pub signal: Option<i32>,
  pub stdout: Vec<u8>,
  pub stderr: Vec<u8>,
  pub stdout_truncated: bool,
  pub stderr_truncated: bool,
  pub error: Option<String>,
  pub cleanup_error: Option<String>,
}
