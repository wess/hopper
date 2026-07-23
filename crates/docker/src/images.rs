//! Image operations: list, inspect, history, tag, remove, prune, search,
//! pull, push, and the save/load pair for air-gapped transfer.

use crate::client::{Client, Req};
use crate::credentials::{
    encode_registry_auth, registry_host, resolve_auth, split_ref, with_default_tag, RegistryAuth,
};
use crate::error::Result;
use bytes::Bytes;
use model::{
    Image, ImageHistoryEntry, ImageSearchResult, InspectResult, PruneReport, PullProgress,
    PushProgress,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct RawImage {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "RepoTags")]
    repo_tags: Option<Vec<String>>,
    #[serde(rename = "RepoDigests")]
    repo_digests: Option<Vec<String>>,
    #[serde(rename = "Created")]
    created: Option<i64>,
    #[serde(rename = "Size")]
    size: Option<i64>,
    #[serde(rename = "Containers")]
    containers: Option<i64>,
    #[serde(rename = "Labels")]
    labels: Option<BTreeMap<String, String>>,
}

fn map_image(i: RawImage) -> Image {
    let repo_tags: Vec<String> = i
        .repo_tags
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.is_empty() && t != "<none>:<none>")
        .collect();
    Image {
        id: i.id,
        dangling: repo_tags.is_empty(),
        repo_tags,
        repo_digests: i.repo_digests.unwrap_or_default(),
        created: i.created.unwrap_or_default(),
        size: i.size.unwrap_or_default(),
        containers: i.containers.unwrap_or(-1),
        labels: i.labels.unwrap_or_default(),
    }
}

pub async fn list(client: &Client, all: bool) -> Result<Vec<Image>> {
    let raw: Vec<RawImage> = client
        .json(
            Req::get("/images/json")
                .flag("all", all)
                .flag("digests", true),
        )
        .await?;
    Ok(raw.into_iter().map(map_image).collect())
}

pub async fn inspect(client: &Client, id: &str) -> Result<InspectResult> {
    client.json(Req::get(format!("/images/{id}/json"))).await
}

#[derive(Debug, Deserialize)]
struct RawHistory {
    #[serde(rename = "Id")]
    id: Option<String>,
    #[serde(rename = "Created")]
    created: Option<i64>,
    #[serde(rename = "CreatedBy")]
    created_by: Option<String>,
    #[serde(rename = "Size")]
    size: Option<i64>,
    #[serde(rename = "Comment")]
    comment: Option<String>,
}

pub async fn history(client: &Client, id: &str) -> Result<Vec<ImageHistoryEntry>> {
    let raw: Vec<RawHistory> = client
        .json(Req::get(format!("/images/{id}/history")))
        .await?;
    Ok(raw
        .into_iter()
        .map(|h| ImageHistoryEntry {
            id: h.id.unwrap_or_default(),
            created: h.created.unwrap_or_default(),
            created_by: h.created_by.unwrap_or_default(),
            size: h.size.unwrap_or_default(),
            comment: h.comment.unwrap_or_default(),
        })
        .collect())
}

pub async fn remove(client: &Client, id: &str, force: bool) -> Result<()> {
    client
        .action(Req::delete(format!("/images/{id}")).flag("force", force))
        .await
}

pub async fn tag(client: &Client, id: &str, repo: &str, tag: &str) -> Result<()> {
    client
        .action(
            Req::post(format!("/images/{id}/tag"))
                .query("repo", repo)
                .query("tag", tag),
        )
        .await
}

#[derive(Debug, Deserialize, Default)]
struct RawPrune {
    #[serde(rename = "ImagesDeleted")]
    deleted: Option<Vec<Value>>,
    #[serde(rename = "SpaceReclaimed")]
    reclaimed: Option<i64>,
}

/// Prune images. `all` also removes unused tagged images, not just dangling
/// ones — the same distinction `docker image prune -a` draws.
pub async fn prune(client: &Client, all: bool) -> Result<PruneReport> {
    let filters = json!({ "dangling": { (!all).to_string(): true } });
    let raw: RawPrune = client
        .json(Req::post("/images/prune").query("filters", filters.to_string()))
        .await?;
    Ok(PruneReport {
        kind: "images".into(),
        removed: raw.deleted.unwrap_or_default().len() as i64,
        reclaimed: raw.reclaimed.unwrap_or_default(),
    })
}

#[derive(Debug, Deserialize)]
struct RawSearch {
    name: Option<String>,
    description: Option<String>,
    star_count: Option<i64>,
    is_official: Option<bool>,
    is_automated: Option<bool>,
}

pub async fn search(client: &Client, term: &str) -> Result<Vec<ImageSearchResult>> {
    let raw: Vec<RawSearch> = client
        .json(
            Req::get("/images/search")
                .query("term", term)
                .query("limit", 25),
        )
        .await?;
    Ok(raw
        .into_iter()
        .map(|r| ImageSearchResult {
            name: r.name.unwrap_or_default(),
            description: r.description.unwrap_or_default(),
            stars: r.star_count.unwrap_or_default(),
            official: r.is_official.unwrap_or(false),
            automated: r.is_automated.unwrap_or(false),
        })
        .collect())
}

/// One frame of the daemon's pull/push progress stream.
#[derive(Debug, Deserialize, Default)]
struct RawProgress {
    id: Option<String>,
    status: Option<String>,
    error: Option<String>,
    #[serde(rename = "progressDetail")]
    detail: Option<ProgressDetail>,
}

#[derive(Debug, Deserialize, Default)]
struct ProgressDetail {
    current: Option<u64>,
    total: Option<u64>,
}

/// The outcome of a transfer, so the caller can report success or failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub ok: bool,
    pub error: Option<String>,
}

/// Pull an image, invoking `on_progress` per status frame.
///
/// The pull is authenticated whenever a credential exists for the registry —
/// that is what raises Docker Hub's anonymous rate limit and unlocks private
/// and GHCR images. With no credential it is an anonymous pull.
pub async fn pull<F>(
    client: &Client,
    request_id: &str,
    reference: &str,
    mut on_progress: F,
) -> Result<Transfer>
where
    F: FnMut(PullProgress),
{
    let from_image = with_default_tag(reference);
    let mut req = Req::post("/images/create")
        .query("fromImage", &from_image)
        .no_timeout();
    if let Some(auth) = resolve_auth(reference).await {
        req = req.header("X-Registry-Auth", encode_registry_auth(&auth));
    }

    let mut error: Option<String> = None;
    client
        .ndjson::<RawProgress, _>(req, |frame| {
            if let Some(e) = frame.error {
                on_progress(PullProgress {
                    request_id: request_id.to_string(),
                    status: e.clone(),
                    done: true,
                    error: Some(e.clone()),
                    ..Default::default()
                });
                error = Some(e);
                return true;
            }
            on_progress(PullProgress {
                request_id: request_id.to_string(),
                id: frame.id,
                status: frame.status.unwrap_or_default(),
                current: frame.detail.as_ref().and_then(|d| d.current),
                total: frame.detail.as_ref().and_then(|d| d.total),
                done: false,
                error: None,
            });
            true
        })
        .await?;

    on_progress(PullProgress {
        request_id: request_id.to_string(),
        status: error.clone().unwrap_or_else(|| "Pull complete".into()),
        done: true,
        error: error.clone(),
        ..Default::default()
    });
    Ok(Transfer {
        ok: error.is_none(),
        error,
    })
}

/// Push an image to its registry, streaming progress like [`pull`].
///
/// The daemon requires an `X-Registry-Auth` header even for an anonymous push,
/// so one is always sent.
pub async fn push<F>(
    client: &Client,
    request_id: &str,
    reference: &str,
    mut on_progress: F,
) -> Result<Transfer>
where
    F: FnMut(PushProgress),
{
    let (name, tag) = split_ref(reference);
    let auth = resolve_auth(reference)
        .await
        .unwrap_or_else(|| RegistryAuth::anonymous(registry_host(reference)));

    let req = Req::post(format!("/images/{name}/push"))
        .query("tag", &tag)
        .header("X-Registry-Auth", encode_registry_auth(&auth))
        .no_timeout();

    let mut error: Option<String> = None;
    client
        .ndjson::<RawProgress, _>(req, |frame| {
            if let Some(e) = frame.error {
                on_progress(PushProgress {
                    request_id: request_id.to_string(),
                    status: e.clone(),
                    done: true,
                    error: Some(e.clone()),
                    ..Default::default()
                });
                error = Some(e);
                return true;
            }
            on_progress(PushProgress {
                request_id: request_id.to_string(),
                id: frame.id,
                status: frame.status.unwrap_or_default(),
                current: frame.detail.as_ref().and_then(|d| d.current),
                total: frame.detail.as_ref().and_then(|d| d.total),
                done: false,
                error: None,
            });
            true
        })
        .await?;

    on_progress(PushProgress {
        request_id: request_id.to_string(),
        status: error.clone().unwrap_or_else(|| "Push complete".into()),
        done: true,
        error: error.clone(),
        ..Default::default()
    });
    Ok(Transfer {
        ok: error.is_none(),
        error,
    })
}

/// Export one or more images as a tar archive, for air-gapped transfer.
pub async fn save(client: &Client, refs: &[String]) -> Result<Bytes> {
    let mut req = Req::get("/images/get").no_timeout();
    for r in refs {
        req = req.query("names", r);
    }
    client.bytes(req).await
}

/// Import images from a tar archive produced by [`save`].
pub async fn load(client: &Client, tar: Bytes) -> Result<()> {
    client
        .action(
            Req::post("/images/load")
                .raw_body(tar, "application/x-tar")
                .no_timeout(),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_tagged_image() {
        let raw: RawImage = serde_json::from_value(json!({
            "Id": "sha256:abc",
            "RepoTags": ["nginx:latest", "nginx:1.25"],
            "RepoDigests": ["nginx@sha256:def"],
            "Created": 1700000000i64,
            "Size": 142000000i64,
            "Containers": 2
        }))
        .unwrap();
        let img = map_image(raw);
        assert_eq!(img.repo_tags.len(), 2);
        assert!(!img.dangling);
        assert_eq!(img.containers, 2);
    }

    #[test]
    fn an_image_with_only_the_none_tag_is_dangling() {
        let raw: RawImage = serde_json::from_value(json!({
            "Id": "sha256:abc",
            "RepoTags": ["<none>:<none>"]
        }))
        .unwrap();
        let img = map_image(raw);
        assert!(img.dangling);
        assert!(img.repo_tags.is_empty());
    }

    #[test]
    fn a_null_repo_tags_field_is_treated_as_dangling_not_a_crash() {
        let raw: RawImage = serde_json::from_value(json!({
            "Id": "sha256:abc",
            "RepoTags": null
        }))
        .unwrap();
        assert!(map_image(raw).dangling);
    }

    #[test]
    fn unknown_container_count_stays_minus_one() {
        let raw: RawImage = serde_json::from_value(json!({"Id": "x"})).unwrap();
        assert_eq!(map_image(raw).containers, -1);
    }

    #[test]
    fn prune_filters_distinguish_dangling_from_all() {
        // `docker image prune` keeps tagged images: dangling=true.
        let dangling = json!({ "dangling": { true.to_string(): true } });
        assert_eq!(dangling["dangling"]["true"], true);
        // `-a` sweeps unused tagged images too: dangling=false.
        let all = json!({ "dangling": { false.to_string(): true } });
        assert_eq!(all["dangling"]["false"], true);
    }
}
