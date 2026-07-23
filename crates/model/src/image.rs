//! Images: the list projection, history, build input, and the pull/push/build
//! progress frames that stream out of the daemon.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub id: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    /// Unix seconds.
    pub created: i64,
    /// Bytes.
    pub size: i64,
    /// -1 when unknown (the daemon only counts with `shared-size`).
    pub containers: i64,
    pub dangling: bool,
    pub labels: BTreeMap<String, String>,
}

impl Image {
    /// The tag to show in a list: first real tag, else a short digest.
    pub fn display_name(&self) -> String {
        match self.repo_tags.iter().find(|t| *t != "<none>:<none>") {
            Some(tag) => tag.clone(),
            None => format!("<none>:{}", self.short_id()),
        }
    }

    pub fn short_id(&self) -> String {
        self.id
            .strip_prefix("sha256:")
            .unwrap_or(&self.id)
            .chars()
            .take(12)
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageHistoryEntry {
    pub id: String,
    pub created: i64,
    pub created_by: String,
    pub size: i64,
    pub comment: String,
}

/// One frame of `docker pull` progress, keyed by layer id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullProgress {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total: Option<u64>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// One frame of `docker push` progress — the same shape family as
/// [`PullProgress`], kept separate so the two streams can't be crossed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushProgress {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub id: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub total: Option<u64>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

/// A Docker Hub search result.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSearchResult {
    pub name: String,
    pub description: String,
    pub stars: i64,
    pub official: bool,
    pub automated: bool,
}

/// Input for `docker build`. `context_dir` is the build context root on the
/// host; `dockerfile` is relative to it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInput {
    pub context_dir: String,
    /// Defaults to "Dockerfile".
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub dockerfile: Option<String>,
    /// `name:tag` to apply to the built image.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tag: Option<String>,
    /// Multi-stage target stage.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target: Option<String>,
    #[serde(default)]
    pub build_args: BTreeMap<String, String>,
    #[serde(default)]
    pub no_cache: bool,
    /// Always attempt to pull a newer base image.
    #[serde(default)]
    pub pull: bool,
    /// Target platform (`linux/amd64`), passed through to BuildKit.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub platform: Option<String>,
}

impl BuildInput {
    pub fn dockerfile_name(&self) -> &str {
        self.dockerfile.as_deref().unwrap_or("Dockerfile")
    }
}

/// One frame of build output. `stream` carries the daemon's build log lines;
/// `image_id` is set on the final aux frame when the build succeeds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProgress {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub image_id: Option<String>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_prefers_a_real_tag() {
        let img = Image {
            id: "sha256:abcdef0123456789".into(),
            repo_tags: vec!["<none>:<none>".into(), "nginx:latest".into()],
            ..Default::default()
        };
        assert_eq!(img.display_name(), "nginx:latest");
    }

    #[test]
    fn display_name_falls_back_to_a_short_digest() {
        let img = Image {
            id: "sha256:abcdef0123456789".into(),
            repo_tags: vec!["<none>:<none>".into()],
            ..Default::default()
        };
        assert_eq!(img.display_name(), "<none>:abcdef012345");
    }

    #[test]
    fn short_id_strips_the_sha_prefix() {
        let img = Image {
            id: "sha256:abcdef0123456789".into(),
            ..Default::default()
        };
        assert_eq!(img.short_id(), "abcdef012345");
    }
}
