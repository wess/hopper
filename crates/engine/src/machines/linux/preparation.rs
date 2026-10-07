#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
  Inspecting,
  Verifying { bytes: u64, total: u64 },
  Downloading { bytes: u64, total: u64 },
  Disk,
  Accounts,
  Media,
  Ready,
}

impl Phase {
  pub fn message(self) -> String {
    match self {
      Self::Inspecting => "Checking Ubuntu installation…".into(),
      Self::Verifying { bytes, total } => amount("Verifying Ubuntu", bytes, total),
      Self::Downloading { bytes, total } => amount("Downloading Ubuntu", bytes, total),
      Self::Disk => "Preparing Ubuntu disk…".into(),
      Self::Accounts => "Preparing Ubuntu accounts…".into(),
      Self::Media => "Preparing Ubuntu installer…".into(),
      Self::Ready => "Starting Ubuntu…".into(),
    }
  }
}

fn amount(action: &str, bytes: u64, total: u64) -> String {
  format!(
    "{action}: {:.1} / {:.1} MiB",
    bytes as f64 / 1048576.0,
    total as f64 / 1048576.0
  )
}
