use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInstallation {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub location: String,
    pub application: Option<String>,
    pub removal: RemovalMethod,
    pub instructions: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RemovalMethod {
    DockerUninstaller {
        application: String,
    },
    Homebrew {
        executable: String,
        package: String,
        cask: bool,
    },
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalReview {
    pub installation: EngineInstallation,
    pub blockers: Vec<String>,
    pub consequences: String,
    pub command: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalConsent {
    pub application_name: String,
    pub backups_verified: bool,
    pub workloads_verified: bool,
}
