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
  Capture {},
  Status {},
  Pause {},
  Resume {},
  Input {
    device: InputDevice,
    events: Vec<InputEvent>,
  },
  Release {},
  Stop {},
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Boot {
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
  Started {},
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", deny_unknown_fields)]
pub enum SetupStatus {
  Waiting {},
  Active { phase: SetupPhase },
  Failed { phase: SetupPhase },
  Deployed {},
  Invalid {},
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
