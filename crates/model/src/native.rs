pub mod keyboard;
pub mod snapshot;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
  pub id: u64,
  pub command: Command,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
  Start {
    boot: Boot,
  },
  StartGeneration {
    boot: Boot,
    restore: Restore,
  },
  StartRestored {
    boot: Boot,
    restore: Restore,
  },
  RestoreResume {
    restore: Restore,
  },
  RestorePause {
    restore: Restore,
  },
  Capture {},
  SnapshotStart {
    #[serde(rename = "snapshotId")]
    snapshot_id: String,
    #[serde(rename = "policyGeneration")]
    policy_generation: u64,
  },
  SnapshotPoll {
    #[serde(rename = "snapshotId")]
    snapshot_id: String,
    #[serde(rename = "policyGeneration")]
    policy_generation: u64,
  },
  SnapshotCancel {
    #[serde(rename = "snapshotId")]
    snapshot_id: String,
    #[serde(rename = "policyGeneration")]
    policy_generation: u64,
  },
  Status {},
  Pause {},
  Resume {},
  Input {
    device: InputDevice,
    events: Vec<InputEvent>,
  },
  Release {},
  /// private installer acknowledgement; never exposed through agent control.
  FinishDeployment {},
  Stop {},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Restore {
  pub snapshot_id: String,
  pub generation_id: String,
  pub policy_generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Boot {
  pub machine_id: String,
  pub firmware: String,
  pub variables: String,
  pub store: String,
  pub boot_media: Option<String>,
  pub installer: Option<String>,
  pub disk: Option<String>,
  pub disk_id: String,
  pub memory: u64,
  pub cpus: u32,
  /// omitted for persistent guests; finite deadlines are for bounded diagnostics.
  #[serde(default)]
  pub timeout_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputDevice {
  Keyboard,
  Tablet,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputEvent {
  pub kind: u16,
  pub code: u16,
  pub value: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Response {
  pub id: u64,
  pub result: Result,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Result {
  /// native hardware is allocated; this does not imply guest desktop readiness.
  Started {
    #[serde(rename = "machineId")]
    machine_id: String,
    configuration: [u8; 32],
  },
  GenerationStarted {
    #[serde(rename = "machineId")]
    machine_id: String,
    configuration: [u8; 32],
    restore: Restore,
  },
  Restored {
    #[serde(rename = "machineId")]
    machine_id: String,
    configuration: [u8; 32],
    restore: Restore,
  },
  Snapshot {
    #[serde(rename = "snapshotId")]
    snapshot_id: String,
    status: snapshot::Status,
  },
  Paused {},
  Running {},
  Accepted {},
  Status {
    paused: bool,
    setup: SetupStatus,
  },
  Frame {
    width: u32,
    height: u32,
    generation: u64,
  },
  Stopped {
    reason: StopReason,
  },
  Rejected {
    message: String,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum SetupStatus {
  Waiting {},
  Active { phase: SetupPhase },
  Failed { phase: SetupPhase },
  Deployed {},
  Invalid {},
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum Installation {
  Preparing {},
  Setup {
    status: SetupStatus,
  },
  Interrupted {
    status: SetupStatus,
  },
  Deployed {},
  Booting {},
  /// hardware has started from the system disk; desktop readiness is still unproven.
  SystemStarted {},
  HandoffFailed {},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
  Requested,
  Shutdown,
  Reset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum SetupPhase {
  Files,
  Pe,
  Drivers,
  Media,
  Image,
  Letters,
  Partition,
  Apply,
  OfflineDrivers,
  Provision,
  Recovery,
  Boot,
}

/// Native boot requests carry the canonical persisted VM identity, never a path-derived guess.
pub fn machine_identity(id: &str) -> Option<[u8; 16]> {
  let identity = uuid::Uuid::parse_str(id).ok()?;
  (!identity.is_nil() && identity.to_string() == id).then(|| *identity.as_bytes())
}
