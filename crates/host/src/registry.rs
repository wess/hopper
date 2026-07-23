//! Searching public registries for images to pull.
//!
//! This talks to each registry's own web API over HTTPS — not the Docker
//! daemon — so it works whether or not an engine is up: you browse first, then
//! pull once you have one. Every hit comes back as a [`RegistryResult`] with a
//! `reference` that is ready to hand straight to `docker pull`.

use anyhow::Context as _;
use model::{RegistryResult, RegistrySource};
use serde::Deserialize;

const HUB_SEARCH: &str = "https://hub.docker.com/v2/search/repositories/";
const GITHUB_SEARCH: &str = "https://api.github.com/search/repositories";
const PAGE: &str = "25";

/// One HTTP client per search. A `User-Agent` is not optional: GitHub rejects
/// requests without one.
fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("hopper/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Search a registry for images matching `query`. An empty query is no search,
/// not an error.
pub async fn search(source: RegistrySource, query: &str) -> anyhow::Result<Vec<RegistryResult>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    match source {
        RegistrySource::DockerHub => docker_hub(query).await,
        RegistrySource::Ghcr => github(query).await,
        // Quay's search API is modelled but not wired up yet; offering it in the
        // UI before it returns real results would be worse than leaving it out.
        RegistrySource::Quay => Ok(Vec::new()),
    }
}

#[derive(Deserialize)]
struct HubResponse {
    results: Vec<HubItem>,
}

#[derive(Deserialize)]
struct HubItem {
    repo_name: String,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    star_count: i64,
    #[serde(default)]
    is_official: bool,
}

async fn docker_hub(query: &str) -> anyhow::Result<Vec<RegistryResult>> {
    let resp: HubResponse = client()?
        .get(HUB_SEARCH)
        .query(&[("query", query), ("page_size", PAGE)])
        .send()
        .await
        .context("searching Docker Hub")?
        .error_for_status()
        .context("Docker Hub search failed")?
        .json()
        .await
        .context("reading the Docker Hub response")?;

    Ok(resp
        .results
        .into_iter()
        .map(|it| {
            // Official images live at /_/name; everything else at /r/owner/name.
            let url = if it.is_official {
                format!("https://hub.docker.com/_/{}", it.repo_name)
            } else {
                format!("https://hub.docker.com/r/{}", it.repo_name)
            };
            RegistryResult {
                source: RegistrySource::DockerHub,
                name: it.repo_name.clone(),
                reference: it.repo_name,
                description: it.short_description.unwrap_or_default(),
                stars: it.star_count,
                official: it.is_official,
                url,
                updated: None,
            }
        })
        .collect())
}

#[derive(Deserialize)]
struct GithubResponse {
    items: Vec<GithubItem>,
}

#[derive(Deserialize)]
struct GithubItem {
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    stargazers_count: i64,
    #[serde(default)]
    html_url: String,
}

async fn github(query: &str) -> anyhow::Result<Vec<RegistryResult>> {
    let resp: GithubResponse = client()?
        .get(GITHUB_SEARCH)
        .query(&[("q", query), ("per_page", PAGE), ("sort", "stars")])
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("searching GitHub")?
        .error_for_status()
        .context("GitHub search failed")?
        .json()
        .await
        .context("reading the GitHub response")?;

    Ok(resp
        .items
        .into_iter()
        .map(|it| RegistryResult {
            source: RegistrySource::Ghcr,
            // GHCR references are always lower-case, unlike the repo name.
            reference: format!("ghcr.io/{}", it.full_name.to_lowercase()),
            name: it.full_name,
            description: it.description.unwrap_or_default(),
            stars: it.stargazers_count,
            official: false,
            url: it.html_url,
            updated: None,
        })
        .collect())
}
