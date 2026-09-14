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
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const LATEST_RELEASE: &str = "https://api.github.com/repos/apple/container/releases/latest";
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;

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
        a.name.ends_with(".pkg")
            && a.name.contains("signed")
            && !a.name.contains("unsigned")
            && !a.name.contains('/')
            && !a.name.contains('\\')
            && trusted_asset_url(&a.browser_download_url)
    })?;
    Some(Installer {
        version: release.tag_name.trim_start_matches('v').to_string(),
        url: asset.browser_download_url.clone(),
        file_name: asset.name.clone(),
    })
}

/// GitHub release metadata is remote input. Restrict the installer to the
/// canonical Apple/container release path before handing it to Installer.app;
/// reqwest may still follow GitHub's normal redirect to its asset CDN.
fn trusted_asset_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url
            .path()
            .starts_with("/apple/container/releases/download/")
}

/// Ask GitHub which installer is current.
pub async fn latest() -> Result<Installer, String> {
    let client = reqwest::Client::builder()
        .user_agent("hopper")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let release: Release = client
        .get(LATEST_RELEASE)
        .send()
        .await
        .map_err(|e| format!("could not reach GitHub: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GitHub did not return a release listing: {e}"))?
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
        .connect_timeout(Duration::from_secs(10))
        // Packages are streamed, so allow a slower connection while still
        // ensuring a stalled download cannot live forever.
        .timeout(Duration::from_secs(15 * 60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(&installer.url)
        .send()
        .await
        .map_err(|e| format!("could not download the installer: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Apple's installer download failed: {e}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_INSTALLER_BYTES)
    {
        return Err(format!(
            "Apple's installer is larger than the {} MiB safety limit.",
            MAX_INSTALLER_BYTES / (1024 * 1024)
        ));
    }

    // Stream to disk instead of buffering a potentially large package in the
    // app. A temporary name prevents Installer.app from seeing a partial
    // package if the connection or the process dies halfway through.
    let partial = path.with_extension("pkg.part");
    let download = async {
        let mut file = tokio::fs::File::create(&partial)
            .await
            .map_err(|e| format!("could not create the installer download: {e}"))?;
        let mut downloaded = 0_u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("the installer download did not complete: {e}"))?
        {
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or_else(|| "Apple's installer download is too large.".to_string())?;
            if downloaded > MAX_INSTALLER_BYTES {
                return Err(format!(
                    "Apple's installer is larger than the {} MiB safety limit.",
                    MAX_INSTALLER_BYTES / (1024 * 1024)
                ));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("could not save the installer: {e}"))?;
        }
        file.flush()
            .await
            .map_err(|e| format!("could not finish saving the installer: {e}"))?;
        Ok::<(), String>(())
    }
    .await;
    if let Err(error) = download {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    if let Err(error) = verify_signed_package(&partial).await {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    tokio::fs::rename(&partial, &path).await.map_err(|e| {
        // Rename can fail independently (for example on a full or
        // read-only Downloads folder); do not leave a misleading partial
        // package behind in that case either.
        let _ = std::fs::remove_file(&partial);
        format!("could not finalize the installer download: {e}")
    })?;

    open(&path).await?;
    Ok(path)
}

/// Verify the package before opening Installer.app. Gatekeeper performs its
/// own checks later, but doing this here lets Hopper fail with a clear message
/// and avoids presenting a tampered or incomplete package to the user.
async fn verify_signed_package(path: &std::path::Path) -> Result<(), String> {
    let mut child = tokio::process::Command::new("/usr/sbin/pkgutil")
        .args(["--check-signature"])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not verify Apple's installer signature: {e}"))?;
    let status = tokio::time::timeout(Duration::from_secs(30), child.wait())
        .await
        .map_err(|_| "Apple's installer signature check timed out.".to_string())?
        .map_err(|e| format!("could not verify Apple's installer signature: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("Apple's installer package signature could not be verified.".into())
    }
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
                    browser_download_url:
                        "https://github.com/apple/container/releases/download/v1.2.2/container-1.2.2-installer-signed.pkg".into(),
                },
            ],
        }
    }

    #[test]
    fn the_signed_package_is_the_one_chosen() {
        // The unsigned package sorts earlier and would trip Gatekeeper.
        let i = choose(&release()).unwrap();
        assert_eq!(
            i.url,
            "https://github.com/apple/container/releases/download/v1.2.2/container-1.2.2-installer-signed.pkg"
        );
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
    fn malformed_signed_assets_are_not_selected() {
        let mut r = release();
        r.assets.insert(
            0,
            Asset {
                name: "nested/container-installer-signed.pkg".into(),
                browser_download_url: "https://x/bad-path".into(),
            },
        );
        r.assets.insert(
            0,
            Asset {
                name: "container-empty-installer-signed.pkg".into(),
                browser_download_url: "  ".into(),
            },
        );
        assert_eq!(
            choose(&r).unwrap().url,
            "https://github.com/apple/container/releases/download/v1.2.2/container-1.2.2-installer-signed.pkg"
        );
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
        assert!(choose(&Release {
            tag_name: "1.0.0".into(),
            assets: vec![]
        })
        .is_none());
    }

    #[test]
    fn installer_urls_must_be_canonical_github_release_assets() {
        assert!(trusted_asset_url(
            "https://github.com/apple/container/releases/download/v1.2.2/container.pkg"
        ));
        assert!(!trusted_asset_url("https://example.com/container.pkg"));
        assert!(!trusted_asset_url(
            "http://github.com/apple/container/releases/download/v1.2.2/container.pkg"
        ));
        assert!(!trusted_asset_url(
            "https://github.com/other/project/releases/download/v1.2.2/container.pkg"
        ));
    }
}
