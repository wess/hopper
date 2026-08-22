//! Getting Apple's runtime onto the machine.
//!
//! `container` ships as a signed installer package that needs administrator
//! rights, which Hopper does not take. So the most it can do is fetch the
//! package Apple signed and hand it to the system installer — the user
//! approves it, as they would any other install.
//!
//! The asset name carries the version (`container-1.2.2-installer-signed.pkg`),
//! so there is no stable `latest/download/…` URL to hardcode; the release has
//! to be resolved through the API.

use serde::Deserialize;
use std::path::PathBuf;

const LATEST_RELEASE: &str = "https://api.github.com/repos/apple/container/releases/latest";

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

/// What Hopper found to install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installer {
    pub version: String,
    pub url: String,
    pub file_name: String,
}

/// Pick the signed installer out of a release's assets.
///
/// Signed only — the release also carries an unsigned package and a debug
/// symbol bundle, and installing an unsigned one would trip Gatekeeper.
fn choose(release: &Release) -> Option<Installer> {
    let asset = release.assets.iter().find(|a| {
        a.name.ends_with(".pkg") && a.name.contains("signed") && !a.name.contains("unsigned")
    })?;
    Some(Installer {
        version: release.tag_name.trim_start_matches('v').to_string(),
        url: asset.browser_download_url.clone(),
        file_name: asset.name.clone(),
    })
}

/// Ask GitHub which installer is current.
pub async fn latest() -> Result<Installer, String> {
    let client = reqwest::Client::builder()
        .user_agent("hopper")
        .build()
        .map_err(|e| e.to_string())?;
    let release: Release = client
        .get(LATEST_RELEASE)
        .send()
        .await
        .map_err(|e| format!("could not reach GitHub: {e}"))?
        .json()
        .await
        .map_err(|e| format!("could not read the release listing: {e}"))?;
    choose(&release).ok_or_else(|| "Apple's latest release has no signed installer.".to_string())
}

/// Download the installer and open it, so macOS asks the user to approve.
///
/// Returns the path it landed on. Written to Downloads because that is where a
/// user expects to find something they were asked to approve — and where they
/// can delete it afterwards.
pub async fn download_and_open() -> Result<PathBuf, String> {
    let installer = latest().await?;
    let dir = downloads_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("could not open the Downloads folder: {e}"))?;
    let path = dir.join(&installer.file_name);

    let client = reqwest::Client::builder()
        .user_agent("hopper")
        .build()
        .map_err(|e| e.to_string())?;
    let bytes = client
        .get(&installer.url)
        .send()
        .await
        .map_err(|e| format!("could not download the installer: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("the download did not complete: {e}"))?;
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| format!("could not save the installer: {e}"))?;

    open(&path).await?;
    Ok(path)
}

/// Hand a path to the system, which for a `.pkg` means Installer.app.
async fn open(path: &std::path::Path) -> Result<(), String> {
    let status = tokio::process::Command::new("/usr/bin/open")
        .arg(path)
        .status()
        .await
        .map_err(|e| format!("could not open the installer: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("macOS refused to open the installer.".into())
    }
}

fn downloads_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> Release {
        Release {
            tag_name: "1.2.2".into(),
            assets: vec![
                Asset {
                    name: "container-dSYM.zip".into(),
                    browser_download_url: "https://x/dsym".into(),
                },
                Asset {
                    name: "container-installer-unsigned.pkg".into(),
                    browser_download_url: "https://x/unsigned".into(),
                },
                Asset {
                    name: "container-1.2.2-installer-signed.pkg".into(),
                    browser_download_url: "https://x/signed".into(),
                },
            ],
        }
    }

    #[test]
    fn the_signed_package_is_the_one_chosen() {
        // The unsigned package sorts earlier and would trip Gatekeeper.
        let i = choose(&release()).unwrap();
        assert_eq!(i.url, "https://x/signed");
        assert_eq!(i.file_name, "container-1.2.2-installer-signed.pkg");
        assert_eq!(i.version, "1.2.2");
    }

    #[test]
    fn a_v_prefixed_tag_still_yields_a_bare_version() {
        let mut r = release();
        r.tag_name = "v1.3.0".into();
        assert_eq!(choose(&r).unwrap().version, "1.3.0");
    }

    #[test]
    fn a_release_with_no_signed_package_is_refused_rather_than_guessed_at() {
        let r = Release {
            tag_name: "1.0.0".into(),
            assets: vec![Asset {
                name: "container-installer-unsigned.pkg".into(),
                browser_download_url: "https://x/unsigned".into(),
            }],
        };
        assert!(choose(&r).is_none());
    }

    #[test]
    fn a_release_with_no_assets_at_all_is_refused() {
        assert!(choose(&Release { tag_name: "1.0.0".into(), assets: vec![] }).is_none());
    }
}
