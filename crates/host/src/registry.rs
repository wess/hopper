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
// Quay takes no page size: it returns ten a page and reports the rest in
// `has_additional`, so asking for more is ignored rather than honoured.
const QUAY_SEARCH: &str = "https://quay.io/api/v1/find/repositories";
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
        RegistrySource::Quay => quay(query).await,
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

#[derive(Deserialize)]
struct QuayResponse {
    #[serde(default)]
    results: Vec<QuayItem>,
}

#[derive(Deserialize)]
struct QuayItem {
    name: String,
    namespace: QuayNamespace,
    /// Null on plenty of repositories, not just empty.
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    is_public: bool,
    /// `/repository/org/name`, relative to quay.io.
    #[serde(default)]
    href: String,
    /// `find/repositories` also answers with organisations and applications.
    #[serde(default)]
    kind: String,
}

#[derive(Deserialize)]
struct QuayNamespace {
    #[serde(default)]
    name: String,
}

async fn quay(query: &str) -> anyhow::Result<Vec<RegistryResult>> {
    let resp: QuayResponse = client()?
        .get(QUAY_SEARCH)
        .query(&[("query", query)])
        .send()
        .await
        .context("searching Quay")?
        .error_for_status()
        .context("Quay search failed")?
        .json()
        .await
        .context("reading the Quay response")?;

    Ok(quay_results(resp))
}

/// Quay hits as pullable references.
///
/// Two things are dropped rather than shown: a hit that is not a repository
/// (the endpoint also answers with organisations), and a private one, which
/// nobody browsing anonymously can pull.
fn quay_results(resp: QuayResponse) -> Vec<RegistryResult> {
    resp.results
        .into_iter()
        .filter(|it| it.kind == "repository" && it.is_public)
        .map(|it| {
            let path = format!("{}/{}", it.namespace.name, it.name);
            RegistryResult {
                source: RegistrySource::Quay,
                reference: format!("quay.io/{path}"),
                description: it.description.unwrap_or_default(),
                url: if it.href.is_empty() {
                    format!("https://quay.io/repository/{path}")
                } else {
                    format!("https://quay.io{}", it.href)
                },
                name: path,
                // Quay publishes no star count, and its `score` is search
                // relevance rather than popularity — showing it as stars would
                // be inventing a number. -1 is what hides the badge.
                stars: -1,
                // No official-image programme to mark.
                official: false,
                updated: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real `find/repositories?query=nginx` response.
    const NGINX: &str = r#"{"results":[
      {"kind":"repository","title":"repo","name":"nginx-unprivileged",
       "namespace":{"title":"org","kind":"organization","name":"nginx"},
       "description":"","is_public":true,"score":4,
       "href":"/repository/nginx/nginx-unprivileged"},
      {"kind":"repository","title":"repo","name":"nginxolm-operator-bundle",
       "namespace":{"title":"org","kind":"organization","name":"openshifttest"},
       "description":null,"is_public":true,"score":4,
       "href":"/repository/openshifttest/nginxolm-operator-bundle"}
    ]}"#;

    fn parsed(json: &str) -> Vec<RegistryResult> {
        quay_results(serde_json::from_str(json).expect("quay response parses"))
    }

    #[test]
    fn a_quay_hit_becomes_a_reference_docker_pull_would_take() {
        let hits = parsed(NGINX);
        assert_eq!(hits[0].reference, "quay.io/nginx/nginx-unprivileged");
        assert_eq!(hits[0].name, "nginx/nginx-unprivileged");
        assert_eq!(
            hits[0].url,
            "https://quay.io/repository/nginx/nginx-unprivileged"
        );
        assert_eq!(hits[0].source, RegistrySource::Quay);
    }

    #[test]
    fn a_null_description_reads_as_empty_rather_than_failing_the_search() {
        // Quay sends `null`, not `""`, on plenty of repositories.
        let hits = parsed(NGINX);
        assert_eq!(hits[1].description, "");
        assert_eq!(hits[1].reference, "quay.io/openshifttest/nginxolm-operator-bundle");
    }

    #[test]
    fn quay_reports_no_stars_rather_than_passing_off_its_relevance_score() {
        // `score` is how well the hit matched, not how popular it is. -1 is
        // what keeps the star badge off the row.
        let hits = parsed(NGINX);
        assert!(hits.iter().all(|h| h.stars == -1));
        assert!(hits.iter().all(|h| !h.official));
    }

    #[test]
    fn private_repositories_and_non_repositories_are_left_out() {
        // The endpoint also answers with organisations, and a private repo is
        // one nobody browsing anonymously could pull.
        let hits = parsed(
            r#"{"results":[
              {"kind":"organization","name":"redhat","namespace":{"name":"redhat"},
               "is_public":true,"href":"/organization/redhat"},
              {"kind":"repository","name":"secret","namespace":{"name":"acme"},
               "is_public":false,"href":"/repository/acme/secret"},
              {"kind":"repository","name":"open","namespace":{"name":"acme"},
               "is_public":true,"href":"/repository/acme/open"}
            ]}"#,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].reference, "quay.io/acme/open");
    }

    #[test]
    fn a_response_with_nothing_in_it_is_no_results_rather_than_an_error() {
        assert!(parsed(r#"{"results":[]}"#).is_empty());
        assert!(parsed("{}").is_empty());
    }
}
