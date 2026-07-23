//! Volumes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
    /// ISO-ish string as the daemon reports it.
    pub created: String,
    pub scope: String,
    pub labels: BTreeMap<String, String>,
    /// Bytes; -1 when unknown (only `system df` reports sizes).
    pub size: i64,
    pub in_use: bool,
}

/// An entry in a volume's contents, listed by the browse helper.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: i64,
    pub dir: bool,
    /// Unix seconds.
    pub modified: i64,
    /// Symbolic mode as the guest reports it (`drwxr-xr-x`).
    pub mode: String,
}
