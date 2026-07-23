//! Registry search and in-app account connections.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RegistrySource {
    #[serde(rename = "dockerhub")]
    DockerHub,
    #[serde(rename = "ghcr")]
    Ghcr,
    #[serde(rename = "quay")]
    Quay,
}

impl RegistrySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DockerHub => "dockerhub",
            Self::Ghcr => "ghcr",
            Self::Quay => "quay",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::DockerHub => "Docker Hub",
            Self::Ghcr => "GitHub",
            Self::Quay => "Quay.io",
        }
    }
}

/// A unified image-search result across registries. `reference` is ready to
/// pull (`nginx`, `ghcr.io/owner/app`, `quay.io/org/img`); `url` is the human
/// web page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryResult {
    pub source: RegistrySource,
    /// Display name (repo/owner).
    pub name: String,
    /// Pullable reference.
    #[serde(rename = "ref")]
    pub reference: String,
    pub description: String,
    /// Popularity signal; -1 when not applicable.
    pub stars: i64,
    pub official: bool,
    pub url: String,
    /// ISO last-updated, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub updated: Option<String>,
}

/// In-app registry sign-in, so push and pull need no `docker login`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryLoginInput {
    /// Registry host; blank or "docker.io" means Docker Hub.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub server: Option<String>,
    pub username: String,
    /// Password, access token, or PAT.
    pub password: String,
}

/// Where a credential lives. Hopper owns its keychain entries; entries read out
/// of the user's `~/.docker/config.json` are surfaced but not owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialSource {
    Hopper,
    Docker,
}

/// One logged-in registry, for the Accounts panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryConnection {
    /// Canonical server address.
    pub server: String,
    /// Friendly name ("Docker Hub", the host, …).
    pub label: String,
    /// Known for keychain credentials; absent for credential helpers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub username: Option<String>,
    pub source: CredentialSource,
}

/// GitHub connection. A stored PAT raises search rate limits, unlocks private
/// repo search, and authenticates GHCR pulls.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubStatus {
    pub connected: bool,
    /// The authenticated account, when connected.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub login: Option<String>,
}
