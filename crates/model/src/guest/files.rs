use super::{Command, Role};
use serde::{Deserialize, Serialize};

pub const CHUNK: usize = 64 << 10;
pub const LIMIT: u64 = 1 << 40;
pub const CREATE_LIMIT: usize = 32 << 10;
pub const UPLOAD_LIMIT: u64 = 1 << 30;
pub const COMMIT: &[u8; 8] = b"HPU1CMIT";
pub const DIRECTORY_LIMIT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stamp {
  pub device: u64,
  pub inode: u64,
  pub size: u64,
  pub modified: (i64, i64),
  pub changed: (i64, i64),
}

impl Stamp {
  pub fn valid(&self) -> bool {
    self.size <= LIMIT
      && self.inode != 0
      && (0..1_000_000_000).contains(&self.modified.1)
      && (0..1_000_000_000).contains(&self.changed.1)
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Read {
  pub path: String,
  pub offset: u64,
  pub length: usize,
  pub stamp: Option<Stamp>,
  pub role: Role,
}

impl Read {
  pub fn command(&self) -> Option<Command> {
    if !self.path.starts_with('/')
      || self.path.len() > 4096
      || self.path.contains('\0')
      || self.offset > LIMIT
      || !(1..=CHUNK).contains(&self.length)
      || self
        .stamp
        .is_some_and(|stamp| !stamp.valid() || self.offset > stamp.size)
    {
      return None;
    }
    let mut args = vec![
      "--read-file".into(),
      self.path.clone(),
      self.offset.to_string(),
      self.length.to_string(),
    ];
    if let Some(stamp) = self.stamp {
      args.push(serde_json::to_string(&stamp).ok()?);
    }
    Some(Command {
      executable: "/usr/libexec/hopperguest".into(),
      args,
      directory: None,
      timeout_seconds: 30,
      role: self.role,
    })
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
  pub stamp: Stamp,
  pub offset: u64,
  pub length: usize,
  pub eof: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Create {
  pub path: String,
  pub bytes: Vec<u8>,
  pub role: Role,
}

impl Create {
  pub fn command(&self) -> Option<Command> {
    let name = self.path.rsplit('/').next()?;
    if !self.path.starts_with('/')
      || self.path.len() > 4096
      || self.path.contains('\0')
      || name.is_empty()
      || name.len() > 255
      || matches!(name, "." | "..")
      || self.bytes.len() > CREATE_LIMIT
    {
      return None;
    }
    Some(Command {
      executable: "/usr/libexec/hopperguest".into(),
      args: vec![
        "--create-file".into(),
        self.path.clone(),
        self.bytes.len().to_string(),
      ],
      directory: None,
      timeout_seconds: 30,
      role: self.role,
    })
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Upload {
  pub path: String,
  pub size: u64,
  pub sha256: String,
  pub timeout_seconds: u16,
  pub role: Role,
}

impl Upload {
  pub fn command(&self) -> Option<Command> {
    let empty = Create {
      path: self.path.clone(),
      bytes: Vec::new(),
      role: self.role,
    };
    empty.command()?;
    if self.size > UPLOAD_LIMIT
      || !(1..=600).contains(&self.timeout_seconds)
      || self.sha256.len() != 64
      || !self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
      return None;
    }
    Some(Command {
      executable: "/usr/libexec/hopperguest".into(),
      args: vec![
        "--upload-file".into(),
        self.path.clone(),
        self.size.to_string(),
      ],
      directory: None,
      timeout_seconds: self.timeout_seconds,
      role: self.role,
    })
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Uploaded {
  pub file: Receipt,
  pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transfer {
  pub job: super::Job,
  pub offset: u64,
  pub size: u64,
  pub committed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Browse {
  pub path: String,
  pub limit: usize,
  pub after: Option<Vec<u8>>,
  pub stamp: Option<Stamp>,
  pub role: Role,
}

impl Browse {
  pub fn command(&self) -> Option<Command> {
    if !self.path.starts_with('/')
      || self.path.len() > 4096
      || self.path.contains('\0')
      || (self.path != "/" && self.path.ends_with('/'))
      || !(1..=DIRECTORY_LIMIT).contains(&self.limit)
      || self.after.as_ref().is_some_and(|name| !valid_name(name))
      || self.stamp.is_some_and(|stamp| !stamp.valid())
    {
      return None;
    }
    Some(Command {
      executable: "/usr/libexec/hopperguest".into(),
      args: vec![
        "--browse-files".into(),
        self.path.clone(),
        self.limit.to_string(),
        serde_json::to_string(&self.after).ok()?,
        serde_json::to_string(&self.stamp).ok()?,
      ],
      directory: None,
      timeout_seconds: 30,
      role: self.role,
    })
  }
}

pub fn valid_name(name: &[u8]) -> bool {
  (1..=255).contains(&name.len())
    && !name.contains(&0)
    && !name.contains(&b'/')
    && name != b"."
    && name != b".."
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
  File,
  Directory,
  Symlink,
  Other,
  Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
  pub name: Vec<u8>,
  pub kind: Kind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Listing {
  pub stamp: Stamp,
  pub entries: Vec<Entry>,
  pub next: Option<Vec<u8>>,
}
