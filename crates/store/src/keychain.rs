//! Hopper-owned secrets, kept in the OS keychain.
//!
//! Unlike the docker-config path (which Hopper only reads), this is what lets
//! a user sign in to a registry or GitHub from inside the app with no CLI.
//!
//! The layout matches what the earlier TypeScript build wrote, so an upgrade
//! keeps existing logins: service `io.wess.hopper`, all registry credentials
//! in one JSON document under `registry.auths`, and the GitHub token under
//! `github.token`.
//!
//! **Stored values must stay newline-free.** The macOS keychain returns
//! values containing a newline hex-encoded, which silently corrupts them on
//! read — so every document is written as single-line JSON.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SERVICE: &str = "io.wess.hopper";
const REGISTRY_KEY: &str = "registry.auths";
const GITHUB_KEY: &str = "github.token";

/// One stored registry credential. `Token` means the secret is a daemon
/// identity token (the `/auth` response gave one); otherwise it is the account
/// password or access token paired with `username`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCred {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub username: Option<String>,
    pub secret: String,
    pub kind: CredKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredKind {
    Password,
    Token,
}

pub type AuthDoc = BTreeMap<String, StoredCred>;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubCred {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub login: Option<String>,
    pub token: String,
}

fn entry(key: &str) -> Option<keyring::Entry> {
    match keyring::Entry::new(SERVICE, key) {
        Ok(e) => Some(e),
        Err(e) => {
            tracing::warn!("keychain unavailable for {key}: {e}");
            None
        }
    }
}

fn read_raw(key: &str) -> Option<String> {
    let e = entry(key)?;
    match e.get_password() {
        Ok(v) => Some(v),
        Err(keyring::Error::NoEntry) => None,
        Err(err) => {
            tracing::warn!("keychain read failed for {key}: {err}");
            None
        }
    }
}

fn write_raw(key: &str, value: &str) -> bool {
    let Some(e) = entry(key) else { return false };
    if let Err(err) = e.set_password(value) {
        tracing::warn!("keychain write failed for {key}: {err}");
        return false;
    }
    true
}

fn delete_raw(key: &str) {
    let Some(e) = entry(key) else { return };
    match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(err) => tracing::warn!("keychain delete failed for {key}: {err}"),
    }
}

/// Serialize without newlines. `serde_json::to_string` never emits them, but
/// this is load-bearing enough on macOS to assert rather than assume.
fn single_line(value: &impl Serialize) -> Option<String> {
    let s = serde_json::to_string(value).ok()?;
    debug_assert!(!s.contains('\n'), "keychain values must be newline-free");
    Some(s.replace('\n', ""))
}

pub fn load_auths() -> AuthDoc {
    read_raw(REGISTRY_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn get_auth(server: &str) -> Option<StoredCred> {
    load_auths().get(server).cloned()
}

pub fn put_auth(server: &str, cred: StoredCred) -> bool {
    let mut doc = load_auths();
    doc.insert(server.to_string(), cred);
    write_doc(doc)
}

pub fn drop_auth(server: &str) -> bool {
    let mut doc = load_auths();
    if doc.remove(server).is_none() {
        return true;
    }
    write_doc(doc)
}

fn write_doc(doc: AuthDoc) -> bool {
    if doc.is_empty() {
        delete_raw(REGISTRY_KEY);
        return true;
    }
    match single_line(&doc) {
        Some(raw) => write_raw(REGISTRY_KEY, &raw),
        None => false,
    }
}

pub fn get_github() -> Option<GithubCred> {
    let raw = read_raw(GITHUB_KEY)?;
    match serde_json::from_str::<GithubCred>(&raw) {
        Ok(cred) if !cred.token.is_empty() => Some(cred),
        // A legacy or hand-written entry is the bare token.
        _ => Some(GithubCred {
            login: None,
            token: raw,
        }),
    }
}

pub fn set_github(cred: &GithubCred) -> bool {
    match single_line(cred) {
        Some(raw) => write_raw(GITHUB_KEY, &raw),
        None => false,
    }
}

pub fn clear_github() {
    delete_raw(GITHUB_KEY);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_documents_serialize_to_a_single_line() {
        let mut doc = AuthDoc::new();
        doc.insert(
            "https://index.docker.io/v1/".into(),
            StoredCred {
                username: Some("someone".into()),
                secret: "a\nsecret\nwith\nnewlines".into(),
                kind: CredKind::Password,
            },
        );
        let raw = single_line(&doc).unwrap();
        assert!(
            !raw.contains('\n'),
            "a newline here is hex-encoded by the macOS keychain and corrupts the value"
        );
        // The escaped form still round-trips.
        let back: AuthDoc = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, doc);
    }

    #[test]
    fn a_github_credential_round_trips() {
        let cred = GithubCred {
            login: Some("wess".into()),
            token: "ghp_example".into(),
        };
        let raw = single_line(&cred).unwrap();
        let back: GithubCred = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, cred);
    }

    #[test]
    fn credential_kinds_use_the_wire_names_the_typescript_build_wrote() {
        let cred = StoredCred {
            username: None,
            secret: "t".into(),
            kind: CredKind::Token,
        };
        assert!(serde_json::to_string(&cred).unwrap().contains(r#""kind":"token""#));
    }
}
