//! Acquiring the guest image when it is not on disk.
//!
//! Hopper's managed engine needs a Linux kernel and initramfs to boot. The
//! release bundles them in `Contents/Resources/`, but they can be absent — a
//! lean install, a stripped bundle, or a first run before they are staged. So
//! rather than dead-end with "no engine", Hopper fetches them from its own
//! GitHub release, verifies them against a published checksum, and caches them
//! under `~/.hopper/` for every subsequent boot.
//!
//! The version is pinned to the running app, so the kernel and the guest init
//! that expects it always match.

use anyhow::Context as _;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const KERNEL: &str = "vmlinuz";
pub const INITRD: &str = "initrd";
pub const MANIFEST: &str = "guest.json";

const RELEASE_BASE: &str = "https://github.com/wess/hopper/releases/download";

/// The root of the download cache, distinct from the bundled copy so a
/// reinstall's bundled image always wins over a stale download.
pub fn cache_dir() -> PathBuf {
    store::paths::engine_dir().join("guest")
}

/// Where a *specific version's* downloaded image is cached. Scoping by version
/// is what keeps an upgrade from booting the previous release's guest: the
/// kernel and the guest init that expects it must always be the matched pair,
/// so v0.8.0 never reuses a v0.7.0 download left on disk.
fn cache_for(version: &str) -> PathBuf {
    cache_dir().join(version)
}

/// The kernel/initrd pair, wherever they live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guest {
    pub kernel: PathBuf,
    pub initrd: PathBuf,
}

impl Guest {
    fn under(dir: &Path) -> Self {
        Guest {
            kernel: dir.join(KERNEL),
            initrd: dir.join(INITRD),
        }
    }

    fn present(&self, exists: impl Fn(&Path) -> bool) -> bool {
        exists(&self.kernel) && exists(&self.initrd)
    }
}

/// Find the guest image for `version`, preferring the bundled copy over the
/// download cache. Returns `None` when neither has it — the signal to acquire.
/// A cache from a *different* version is ignored, not reused.
pub fn locate(bundle: &Path, version: &str, exists: impl Fn(&Path) -> bool + Copy) -> Option<Guest> {
    let bundled = Guest::under(bundle);
    if bundled.present(exists) {
        return Some(bundled);
    }
    let cached = Guest::under(&cache_for(version));
    cached.present(exists).then_some(cached)
}

/// The base URL to fetch guest assets from. `HOPPER_ENGINE_BASE` overrides the
/// GitHub release — for an air-gapped mirror, or for tests.
fn release_base() -> String {
    match std::env::var("HOPPER_ENGINE_BASE") {
        Ok(base) if !base.trim().is_empty() => base.trim_end_matches('/').to_string(),
        _ => RELEASE_BASE.to_string(),
    }
}

/// The download URL for a release asset.
pub fn asset_url(version: &str, file: &str) -> String {
    format!("{}/v{version}/{file}", release_base())
}

/// The published integrity manifest for a version's guest image.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub kernel: AssetInfo,
    pub initrd: AssetInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetInfo {
    pub sha256: String,
    pub size: u64,
}

impl Manifest {
    pub fn total_size(&self) -> u64 {
        self.kernel.size + self.initrd.size
    }
}

/// Hex SHA-256 of some bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Verify a downloaded file against its manifest entry. A partial or tampered
/// download must never be booted, so both size and checksum are checked.
pub fn verify(path: &Path, info: &AssetInfo) -> anyhow::Result<()> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {} for verification", path.display()))?;
    anyhow::ensure!(
        bytes.len() as u64 == info.size,
        "{} is {} bytes, expected {}",
        path.display(),
        bytes.len(),
        info.size
    );
    let got = sha256_hex(&bytes);
    anyhow::ensure!(
        got.eq_ignore_ascii_case(&info.sha256),
        "{} failed its checksum — the download is corrupt",
        path.display()
    );
    Ok(())
}

/// Download a URL to `dest`, reporting `(bytes_so_far, total)` where
/// `bytes_so_far` is offset by `base` so a caller can sum multiple files.
#[cfg(target_os = "macos")]
async fn download(
    url: &str,
    dest: &Path,
    base: u64,
    total: u64,
    on_progress: &mut impl FnMut(u64, u64),
) -> anyhow::Result<()> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let resp = reqwest::get(url)
        .await
        .with_context(|| format!("requesting {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} is not available"))?;

    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("creating {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("the download was interrupted")?;
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
        on_progress(base + written, total);
    }
    file.flush().await?;
    Ok(())
}

/// Download and cache the guest image for `version`, verifying each file, and
/// return where it landed. Downloads to `.part` files and renames on success,
/// so an interrupted acquisition never leaves a half-written image that looks
/// present.
#[cfg(target_os = "macos")]
pub async fn acquire(
    version: &str,
    mut on_progress: impl FnMut(u64, u64),
) -> anyhow::Result<Guest> {
    let dir = cache_for(version);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("creating {}", dir.display()))?;

    let manifest: Manifest = reqwest::get(asset_url(version, MANIFEST))
        .await
        .context("requesting the guest manifest")?
        .error_for_status()
        .context("no guest image is published for this version")?
        .json()
        .await
        .context("the guest manifest could not be read")?;
    let total = manifest.total_size();

    let kernel_tmp = dir.join(format!("{KERNEL}.part"));
    let initrd_tmp = dir.join(format!("{INITRD}.part"));

    download(&asset_url(version, KERNEL), &kernel_tmp, 0, total, &mut on_progress).await?;
    verify(&kernel_tmp, &manifest.kernel)?;

    download(
        &asset_url(version, INITRD),
        &initrd_tmp,
        manifest.kernel.size,
        total,
        &mut on_progress,
    )
    .await?;
    verify(&initrd_tmp, &manifest.initrd)?;

    let guest = Guest::under(&dir);
    tokio::fs::rename(&kernel_tmp, &guest.kernel).await?;
    tokio::fs::rename(&initrd_tmp, &guest.initrd).await?;
    on_progress(total, total);
    Ok(guest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_point_at_the_versioned_release() {
        assert_eq!(
            asset_url("0.7.0", KERNEL),
            "https://github.com/wess/hopper/releases/download/v0.7.0/vmlinuz"
        );
        assert_eq!(
            asset_url("0.7.0", MANIFEST),
            "https://github.com/wess/hopper/releases/download/v0.7.0/guest.json"
        );
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        // SHA-256 of the empty string.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"hopper"),
            sha256_hex(b"hopper"),
            "hashing is deterministic"
        );
    }

    #[test]
    fn locate_prefers_the_bundled_image_over_the_cache() {
        // Bundle has both files → bundle wins.
        let bundle = PathBuf::from("/App.app/Contents/Resources");
        let found = locate(&bundle, "0.8.0", |_| true).unwrap();
        assert_eq!(found.kernel, bundle.join(KERNEL));
    }

    #[test]
    fn locate_falls_back_to_the_versioned_cache() {
        let bundle = PathBuf::from("/App.app/Contents/Resources");
        // Only this version's cache files exist.
        let cache = cache_for("0.8.0");
        let found = locate(&bundle, "0.8.0", |p| p.starts_with(&cache)).unwrap();
        assert!(found.kernel.starts_with(&cache));
    }

    #[test]
    fn locate_ignores_a_cache_from_another_version() {
        // A v0.7.0 download left on disk must not satisfy a v0.8.0 lookup —
        // the kernel and its guest init are a matched pair.
        let old = cache_for("0.7.0");
        let found = locate(&PathBuf::from("/nowhere"), "0.8.0", |p| p.starts_with(&old));
        assert!(found.is_none());
    }

    #[test]
    fn locate_returns_none_when_neither_has_the_image() {
        assert!(locate(&PathBuf::from("/nowhere"), "0.8.0", |_| false).is_none());
    }

    #[test]
    fn verify_rejects_a_size_mismatch() {
        let dir = std::env::temp_dir().join(format!("hopperacq{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("k");
        std::fs::write(&f, b"1234").unwrap();
        let info = AssetInfo {
            sha256: sha256_hex(b"1234"),
            size: 99, // wrong
        };
        assert!(verify(&f, &info).unwrap_err().to_string().contains("expected 99"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_rejects_a_checksum_mismatch() {
        let dir = std::env::temp_dir().join(format!("hopperacq2{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("k");
        std::fs::write(&f, b"real content").unwrap();
        let info = AssetInfo {
            sha256: sha256_hex(b"different content"),
            size: 12,
        };
        assert!(verify(&f, &info).unwrap_err().to_string().contains("checksum"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_accepts_a_matching_file() {
        let dir = std::env::temp_dir().join(format!("hopperacq3{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("k");
        let content = b"exactly this";
        std::fs::write(&f, content).unwrap();
        let info = AssetInfo {
            sha256: sha256_hex(content),
            size: content.len() as u64,
        };
        assert!(verify(&f, &info).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_parses_and_totals_its_assets() {
        let m: Manifest = serde_json::from_str(
            r#"{"kernel":{"sha256":"aa","size":100},"initrd":{"sha256":"bb","size":250}}"#,
        )
        .unwrap();
        assert_eq!(m.total_size(), 350);
        assert_eq!(m.kernel.sha256, "aa");
    }
}
