use serde::{Deserialize, Serialize};

/// Private worker/host handoff only; it conveys no policy or runtime authority.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
  pub nonce: [u8; 16],
  pub device: u64,
  pub inode: u64,
  pub manifest: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum Status {
  Capturing {},
  Staging {},
  Ready { receipt: Receipt },
  Failed {},
}
