use anyhow::ensure;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
  Prepared,
  Installing,
  Deployed,
  Failed,
}

impl Phase {
  pub fn message(self) -> &'static str {
    match self {
      Self::Prepared => "Waiting for the Ubuntu installer",
      Self::Installing => "Installing Ubuntu",
      Self::Deployed => "Ubuntu deployment finished; system boot is pending",
      Self::Failed => "Ubuntu installation failed; its disk is preserved for recovery",
    }
  }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
  version: u32,
  attempt: String,
  phase: Phase,
}

pub struct Decoder {
  prefix: Vec<u8>,
  line: Vec<u8>,
  oversized: bool,
  phase: Phase,
}

impl Decoder {
  pub fn new(attempt: &str) -> anyhow::Result<Self> {
    ensure!(
      Uuid::parse_str(attempt)?.to_string() == attempt,
      "Invalid installation attempt"
    );
    Ok(Self {
      prefix: format!("HOPPER-INSTALL:{attempt}:").into_bytes(),
      line: Vec::with_capacity(128),
      oversized: false,
      phase: Phase::Prepared,
    })
  }

  pub fn phase(&self) -> Phase {
    self.phase
  }

  pub fn feed(&mut self, input: &[u8]) -> Vec<Phase> {
    let mut changes = Vec::new();
    for byte in input {
      if *byte == b'\n' {
        if !self.oversized {
          if self.line.last() == Some(&b'\r') {
            self.line.pop();
          }
          let next = self
            .line
            .strip_prefix(self.prefix.as_slice())
            .and_then(|value| match value {
              b"installing" if self.phase == Phase::Prepared => Some(Phase::Installing),
              b"deployed" if self.phase == Phase::Installing => Some(Phase::Deployed),
              b"failed"
                if matches!(
                  self.phase,
                  Phase::Prepared | Phase::Installing | Phase::Deployed
                ) =>
              {
                Some(Phase::Failed)
              }
              _ => None,
            });
          if let Some(next) = next {
            self.phase = next;
            changes.push(next);
          }
        }
        self.line.clear();
        self.oversized = false;
      } else if !self.oversized {
        if self.line.len() == 128 {
          self.line.clear();
          self.oversized = true;
        } else {
          self.line.push(*byte);
        }
      }
    }
    changes
  }
}

pub(super) fn save(directory: &Path, attempt: &str, phase: Phase) -> anyhow::Result<()> {
  super::super::vz::files::directory(directory)?;
  Decoder::new(attempt)?;
  let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
  serde_json::to_writer(
    &mut temporary,
    &Record {
      version: 1,
      attempt: attempt.into(),
      phase,
    },
  )?;
  temporary.as_file().sync_all()?;
  temporary.persist(directory.join("installation"))?;
  std::fs::File::open(directory)?.sync_all()?;
  Ok(())
}

pub fn read(directory: &Path) -> anyhow::Result<Option<Phase>> {
  super::super::vz::files::directory(directory)?;
  let path = directory.join("installation");
  match std::fs::symlink_metadata(&path) {
    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
    Err(error) => return Err(error.into()),
    Ok(_) => {}
  }
  let file = super::super::vz::files::read(&path, 1024)?;
  let record: Record = serde_json::from_reader(file)?;
  ensure!(record.version == 1, "Unknown installation journal version");
  Decoder::new(&record.attempt)?;
  Ok(Some(record.phase))
}
