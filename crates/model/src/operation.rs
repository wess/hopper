use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationState {
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

/// Deliberately excludes command arguments, paths, environment and credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub id: String,
    pub action: String,
    pub state: OperationState,
    pub started: u64,
    pub finished: Option<u64>,
}
