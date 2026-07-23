//! Registry credentials for push and pull.
//!
//! Resolution order, most specific first:
//!
//! 1. **Hopper's keychain** — what the user signed in with inside the app.
//!    This is the path that makes push/pull work with no `docker login`.
//! 2. **A connected GitHub account** for `ghcr.io`, where the PAT doubles as
//!    the registry password.
//! 3. **The user's `~/.docker/config.json`** — credential helpers first (the
//!    secure path), then plaintext `auths` entries.
//!
//! Both push *and* pull send `X-Registry-Auth`: an authenticated pull lifts
//! Docker Hub's anonymous rate limit and unlocks private and GHCR images.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use store::keychain::{self, CredKind};

/// Docker Hub credentials live under this canonical key, not under "docker.io".
pub const DOCKER_HUB_SERVER: &str = "https://index.docker.io/v1/";

/// What the daemon wants in the base64 `X-Registry-Auth` header.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryAuth {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub identitytoken: Option<String>,
    pub serveraddress: String,
}

impl RegistryAuth {
    pub fn anonymous(server: impl Into<String>) -> Self {
        Self {
            serveraddress: server.into(),
            ..Default::default()
        }
    }
}

/// Strip scheme and trailing slashes so `https://host/` and `host` compare
/// equal.
pub fn bare(s: &str) -> String {
    let s = s
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    s.trim_end_matches('/').to_string()
}

const HUB_ALIASES: [&str; 5] = [
    "",
    "docker.io",
    "index.docker.io",
    // The docker config key, scheme- and slash-stripped.
    "index.docker.io/v1",
    "registry-1.docker.io",
];

/// Normalize a user-entered registry host to the key everything else uses.
/// Hub aliases collapse to [`DOCKER_HUB_SERVER`]; any other host stays bare.
pub fn canonical_server(server: &str) -> String {
    let b = bare(server);
    if HUB_ALIASES.contains(&b.as_str()) {
        DOCKER_HUB_SERVER.to_string()
    } else {
        b
    }
}

/// Derive the registry server address from an image reference.
///
/// Bare names and `docker.io/…` map to Hub's canonical endpoint; otherwise the
/// first path segment is a registry host when it has a dot, a port colon, or
/// is `localhost`.
pub fn registry_host(reference: &str) -> String {
    let Some(slash) = reference.find('/') else {
        return DOCKER_HUB_SERVER.to_string();
    };
    let head = &reference[..slash];
    if head == "docker.io" {
        return DOCKER_HUB_SERVER.to_string();
    }
    if head.contains('.') || head.contains(':') || head == "localhost" {
        return head.to_string();
    }
    DOCKER_HUB_SERVER.to_string()
}

/// Split `registry/name:tag` into its push name (no tag) and tag.
///
/// The tag is the part after the last colon, but only when that colon follows
/// the last slash — otherwise a `host:port/name` registry port would be
/// mistaken for a tag.
pub fn split_ref(reference: &str) -> (String, String) {
    let last_slash = reference.rfind('/').map(|i| i as isize).unwrap_or(-1);
    match reference.rfind(':') {
        Some(colon) if (colon as isize) > last_slash => (
            reference[..colon].to_string(),
            reference[colon + 1..].to_string(),
        ),
        _ => (reference.to_string(), "latest".to_string()),
    }
}

/// Add `:latest` when a reference carries no tag or digest.
pub fn with_default_tag(reference: &str) -> String {
    let last_slash = reference.rfind('/').map(|i| i as isize).unwrap_or(-1);
    let has_tag = reference
        .rfind(':')
        .is_some_and(|c| (c as isize) > last_slash);
    if has_tag || reference.contains('@') {
        reference.to_string()
    } else {
        format!("{reference}:latest")
    }
}

/// Base64-encode an auth object for the `X-Registry-Auth` header.
pub fn encode_registry_auth(auth: &RegistryAuth) -> String {
    let json = serde_json::to_vec(auth).unwrap_or_else(|_| b"{}".to_vec());
    base64::engine::general_purpose::URL_SAFE.encode(json)
}

/// Decode a docker config `auth` entry, which is `base64(user:pass)`.
pub fn decode_auth_entry(b64: &str) -> (Option<String>, Option<String>) {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) else {
        return (None, None);
    };
    let Ok(decoded) = String::from_utf8(bytes) else {
        return (None, None);
    };
    match decoded.find(':') {
        Some(i) => (
            Some(decoded[..i].to_string()),
            Some(decoded[i + 1..].to_string()),
        ),
        None => (None, None),
    }
}

/// Find the config key matching a server, comparing scheme- and
/// slash-insensitively.
pub fn match_key<'a>(keys: impl IntoIterator<Item = &'a String>, server: &str) -> Option<String> {
    let want = bare(server);
    let mut fallback = None;
    for k in keys {
        if k == server {
            return Some(k.clone());
        }
        if fallback.is_none() && bare(k) == want {
            fallback = Some(k.clone());
        }
    }
    fallback
}

#[derive(Debug, Default, Deserialize)]
pub struct DockerConfig {
    #[serde(default)]
    pub auths: BTreeMap<String, AuthEntry>,
    #[serde(rename = "credsStore", default)]
    pub creds_store: Option<String>,
    #[serde(rename = "credHelpers", default)]
    pub cred_helpers: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct AuthEntry {
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default)]
    pub identitytoken: Option<String>,
}

pub fn parse_docker_config(text: &str) -> DockerConfig {
    serde_json::from_str(text).unwrap_or_default()
}

fn config_path() -> PathBuf {
    match std::env::var("DOCKER_CONFIG") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir).join("config.json"),
        _ => dirs::home_dir()
            .unwrap_or_default()
            .join(".docker")
            .join("config.json"),
    }
}

fn read_config() -> DockerConfig {
    std::fs::read_to_string(config_path())
        .map(|t| parse_docker_config(&t))
        .unwrap_or_default()
}

/// Ask a credential helper (`docker-credential-<helper> get`) for a secret.
async fn cred_helper_get(helper: &str, server: &str) -> Option<RegistryAuth> {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut child = Command::new(format!("docker-credential-{helper}"))
        .arg("get")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(server.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    let out = child.wait_with_output().await.ok()?;
    if !out.status.success() {
        return None;
    }

    #[derive(Deserialize)]
    struct HelperOut {
        #[serde(rename = "Username")]
        username: Option<String>,
        #[serde(rename = "Secret")]
        secret: Option<String>,
    }
    let c: HelperOut = serde_json::from_slice(&out.stdout).ok()?;
    if c.username.is_none() && c.secret.is_none() {
        return None;
    }
    // Helpers signal token auth with the sentinel username "<token>".
    if c.username.as_deref() == Some("<token>") {
        return Some(RegistryAuth {
            identitytoken: c.secret,
            serveraddress: server.to_string(),
            ..Default::default()
        });
    }
    Some(RegistryAuth {
        username: c.username,
        password: c.secret,
        serveraddress: server.to_string(),
        ..Default::default()
    })
}

/// Resolve credentials for an image reference, or `None` for an anonymous pull.
pub async fn resolve_auth(reference: &str) -> Option<RegistryAuth> {
    let server = registry_host(reference);

    // 1. A credential the user signed in with from inside Hopper.
    if let Some(stored) = keychain::get_auth(&server) {
        return Some(match stored.kind {
            CredKind::Token => RegistryAuth {
                identitytoken: Some(stored.secret),
                serveraddress: server,
                ..Default::default()
            },
            CredKind::Password => RegistryAuth {
                username: stored.username,
                password: Some(stored.secret),
                serveraddress: server,
                ..Default::default()
            },
        });
    }

    // 2. GHCR authenticated by a connected GitHub account.
    if server == "ghcr.io" {
        if let Some(gh) = keychain::get_github() {
            if !gh.token.is_empty() {
                return Some(RegistryAuth {
                    username: Some(gh.login.unwrap_or_else(|| "x-access-token".into())),
                    password: Some(gh.token),
                    serveraddress: server,
                    ..Default::default()
                });
            }
        }
    }

    // 3. The user's existing docker config.
    let cfg = read_config();
    let helper = match_key(cfg.cred_helpers.keys(), &server)
        .and_then(|k| cfg.cred_helpers.get(&k).cloned())
        .or_else(|| cfg.creds_store.clone());
    if let Some(helper) = helper.filter(|h| !h.is_empty()) {
        if let Some(auth) = cred_helper_get(&helper, &server).await {
            return Some(auth);
        }
    }

    let key = match_key(cfg.auths.keys(), &server)?;
    let entry = cfg.auths.get(&key)?;
    if let Some(token) = entry.identitytoken.clone().filter(|t| !t.is_empty()) {
        return Some(RegistryAuth {
            identitytoken: Some(token),
            serveraddress: server,
            ..Default::default()
        });
    }
    let b64 = entry.auth.as_deref().filter(|a| !a.is_empty())?;
    let (username, password) = decode_auth_entry(b64);
    username.as_ref()?;
    Some(RegistryAuth {
        username,
        password,
        serveraddress: server,
        ..Default::default()
    })
}

/// Logged-in registry hosts from the docker config — names only, no secrets.
pub fn list_docker_registries() -> Vec<String> {
    let cfg = read_config();
    let mut set: Vec<String> = cfg
        .auths
        .keys()
        .chain(cfg.cred_helpers.keys())
        .cloned()
        .collect();
    set.sort();
    set.dedup();
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_strips_scheme_and_trailing_slashes() {
        assert_eq!(bare("https://index.docker.io/v1/"), "index.docker.io/v1");
        assert_eq!(bare("http://reg.example.com"), "reg.example.com");
        assert_eq!(bare("  reg.example.com/  "), "reg.example.com");
    }

    #[test]
    fn hub_aliases_all_collapse_to_one_key() {
        for alias in [
            "",
            "docker.io",
            "index.docker.io",
            "https://index.docker.io/v1/",
            "registry-1.docker.io",
        ] {
            assert_eq!(canonical_server(alias), DOCKER_HUB_SERVER, "alias {alias}");
        }
    }

    #[test]
    fn a_private_registry_keeps_its_own_key() {
        assert_eq!(canonical_server("ghcr.io"), "ghcr.io");
        assert_eq!(canonical_server("https://quay.io/"), "quay.io");
    }

    #[test]
    fn registry_host_maps_bare_names_to_hub() {
        assert_eq!(registry_host("nginx"), DOCKER_HUB_SERVER);
        assert_eq!(registry_host("library/nginx"), DOCKER_HUB_SERVER);
        assert_eq!(registry_host("docker.io/library/nginx"), DOCKER_HUB_SERVER);
    }

    #[test]
    fn registry_host_recognizes_real_registries() {
        assert_eq!(registry_host("ghcr.io/owner/app"), "ghcr.io");
        assert_eq!(registry_host("quay.io/org/img"), "quay.io");
        assert_eq!(registry_host("localhost/img"), "localhost");
        assert_eq!(registry_host("localhost:5000/img"), "localhost:5000");
        assert_eq!(registry_host("reg.internal:5000/a/b"), "reg.internal:5000");
    }

    #[test]
    fn split_ref_does_not_mistake_a_registry_port_for_a_tag() {
        assert_eq!(
            split_ref("localhost:5000/app"),
            ("localhost:5000/app".into(), "latest".into())
        );
        assert_eq!(
            split_ref("localhost:5000/app:v2"),
            ("localhost:5000/app".into(), "v2".into())
        );
        assert_eq!(split_ref("nginx"), ("nginx".into(), "latest".into()));
        assert_eq!(split_ref("nginx:1.25"), ("nginx".into(), "1.25".into()));
    }

    #[test]
    fn default_tag_is_added_only_when_missing() {
        assert_eq!(with_default_tag("nginx"), "nginx:latest");
        assert_eq!(with_default_tag("nginx:1.25"), "nginx:1.25");
        assert_eq!(with_default_tag("ghcr.io/o/a"), "ghcr.io/o/a:latest");
        assert_eq!(with_default_tag("localhost:5000/a"), "localhost:5000/a:latest");
        // A digest reference is already fully qualified.
        assert_eq!(with_default_tag("nginx@sha256:abc"), "nginx@sha256:abc");
    }

    #[test]
    fn auth_entries_decode_to_a_username_and_password() {
        let b64 = base64::engine::general_purpose::STANDARD.encode("someone:s3cret");
        assert_eq!(
            decode_auth_entry(&b64),
            (Some("someone".into()), Some("s3cret".into()))
        );
    }

    #[test]
    fn a_password_containing_a_colon_survives_decoding() {
        let b64 = base64::engine::general_purpose::STANDARD.encode("someone:a:b:c");
        assert_eq!(
            decode_auth_entry(&b64),
            (Some("someone".into()), Some("a:b:c".into()))
        );
    }

    #[test]
    fn a_malformed_auth_entry_decodes_to_nothing_rather_than_panicking() {
        assert_eq!(decode_auth_entry("not base64!!"), (None, None));
        let no_colon = base64::engine::general_purpose::STANDARD.encode("nocolon");
        assert_eq!(decode_auth_entry(&no_colon), (None, None));
    }

    #[test]
    fn match_key_prefers_an_exact_hit_over_a_normalized_one() {
        let keys: Vec<String> = vec![
            "https://index.docker.io/v1/".into(),
            "index.docker.io/v1".into(),
        ];
        assert_eq!(
            match_key(keys.iter(), "index.docker.io/v1").as_deref(),
            Some("index.docker.io/v1")
        );
    }

    #[test]
    fn match_key_falls_back_to_scheme_insensitive_comparison() {
        let keys: Vec<String> = vec!["https://index.docker.io/v1/".into()];
        assert_eq!(
            match_key(keys.iter(), "index.docker.io/v1").as_deref(),
            Some("https://index.docker.io/v1/")
        );
    }

    #[test]
    fn match_key_returns_nothing_when_no_key_matches() {
        let keys: Vec<String> = vec!["ghcr.io".into()];
        assert!(match_key(keys.iter(), "quay.io").is_none());
    }

    #[test]
    fn registry_auth_encodes_to_base64_json() {
        let auth = RegistryAuth {
            username: Some("u".into()),
            password: Some("p".into()),
            serveraddress: "ghcr.io".into(),
            ..Default::default()
        };
        let encoded = encode_registry_auth(&auth);
        let decoded = base64::engine::general_purpose::URL_SAFE
            .decode(&encoded)
            .unwrap();
        let back: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(back["username"], "u");
        assert_eq!(back["serveraddress"], "ghcr.io");
        // Absent fields stay absent rather than becoming null.
        assert!(back.get("identitytoken").is_none());
    }

    #[test]
    fn a_docker_config_parses_helpers_and_auths() {
        let cfg = parse_docker_config(
            r#"{
                "auths": {"https://index.docker.io/v1/": {"auth": "dTpw"}},
                "credsStore": "desktop",
                "credHelpers": {"ghcr.io": "gh"}
            }"#,
        );
        assert_eq!(cfg.creds_store.as_deref(), Some("desktop"));
        assert_eq!(cfg.cred_helpers.get("ghcr.io").map(String::as_str), Some("gh"));
        assert!(cfg.auths.contains_key("https://index.docker.io/v1/"));
    }

    #[test]
    fn a_malformed_docker_config_reads_as_empty_rather_than_failing_the_pull() {
        let cfg = parse_docker_config("{ not json");
        assert!(cfg.auths.is_empty());
        assert!(cfg.creds_store.is_none());
    }
}
