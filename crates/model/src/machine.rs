use serde::{Deserialize, Serialize};

use crate::EngineResources;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuestOs {
    Linux,
    Macos,
    Windows,
}

impl GuestOs {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Macos => "macOS",
            Self::Windows => "Windows",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub guest: GuestOs,
    pub profile: String,
    pub resources: EngineResources,
    #[serde(default)]
    pub agent_access: bool,
    #[serde(default)]
    pub installer: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineStatus {
    pub machine: Machine,
    pub state: String,
    pub busy: bool,
    #[serde(default)]
    pub progress: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineProfile {
    pub id: String,
    pub name: String,
    pub guest: GuestOs,
    pub description: String,
    pub download_url: Option<String>,
    pub installer_required: bool,
    pub experimental: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMachine {
    pub name: String,
    pub profile: String,
    pub resources: EngineResources,
    #[serde(default)]
    pub installer: Option<String>,
    #[serde(default = "enabled")]
    pub agent_access: bool,
}

fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSnapshot {
    pub id: String,
    pub name: String,
    pub machine_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MachineInput {
    Key {
        keys: String,
    },
    Text {
        text: String,
    },
    Pointer {
        x: u16,
        y: u16,
        button: Option<String>,
    },
}
