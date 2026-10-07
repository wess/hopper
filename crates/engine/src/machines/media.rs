pub mod download;

use anyhow::{bail, Context};
use fs2::FileExt;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Media {
    pub language_code: String,
    pub architecture: String,
    pub edition: String,
    pub size: u64,
    pub sha1: String,
    pub file_path: String,
}

pub fn catalogue(xml: &str) -> anyhow::Result<Media> {
    #[derive(Deserialize)]
    struct Root {
        #[serde(rename = "Catalogs")]
        catalogs: Catalogs,
    }
    #[derive(Deserialize)]
    struct Catalogs {
        #[serde(rename = "Catalog")]
        catalog: Catalog,
    }
    #[derive(Deserialize)]
    struct Catalog {
        #[serde(rename = "PublishedMedia")]
        media: Published,
    }
    #[derive(Deserialize)]
    struct Published {
        #[serde(rename = "Files")]
        files: Files,
    }
    #[derive(Deserialize)]
    struct Files {
        #[serde(rename = "File", default)]
        files: Vec<Media>,
    }
    let root: Root = quick_xml::de::from_str(xml)?;
    let media = root
        .catalogs
        .catalog
        .media
        .files
        .files
        .into_iter()
        .find(|m| {
            m.architecture == "ARM64" && m.language_code == "en-us" && m.edition == "Professional"
        })
        .context("Microsoft's catalogue has no English US ARM64 Windows installer")?;
    let url = reqwest::Url::parse(&media.file_path)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str() != Some("dl.delivery.mp.microsoft.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || media.sha1.len() != 40
        || !media.sha1.bytes().all(|c| c.is_ascii_hexdigit())
        || !(1024 * 1024 * 1024..=12 * 1024 * 1024 * 1024).contains(&media.size)
    {
        bail!("Microsoft's installer catalogue has invalid download metadata");
    }
    Ok(media)
}

async fn command(program: &Path, args: &[&str]) -> anyhow::Result<std::process::Output> {
    let child = tokio::process::Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let output = tokio::time::timeout(Duration::from_secs(30 * 60), child.wait_with_output())
        .await
        .context("Windows installer preparation timed out")??;
    if !output.status.success() {
        bail!(
            "Installer preparation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}

fn tool(name: &str) -> anyhow::Result<PathBuf> {
    super::cli::runtime_paths()
        .iter()
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .with_context(|| format!("Hopper's Windows installer helper {name} is missing"))
}

pub async fn prepare(root: &Path, progress: &Path) -> anyhow::Result<PathBuf> {
    let cache = root.join("images/windows");
    std::fs::create_dir_all(&cache)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache.join("lock"))?;
    lock.try_lock_exclusive()
        .context("Another VM is preparing the Windows installer; retry when it finishes")?;
    let iso = cache.join("installer.iso");
    if iso.is_file() && iso.metadata()?.len() > 1024 * 1024 * 1024 {
        return Ok(iso);
    }
    let wim = tool("wimlib-imagex")?;
    let mkisofs = tool("mkisofs")?;
    if fs2::available_space(&cache)? < 20 * 1024 * 1024 * 1024 {
        bail!("Windows installer preparation needs 20 GiB of free disk space");
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()?;
    std::fs::write(progress, "Fetching Windows installer catalogue…")?;
    let cab = client
        .get("https://go.microsoft.com/fwlink?linkid=2156292")
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if cab.len() > 4 * 1024 * 1024 {
        bail!("Windows catalogue is too large");
    }
    let stage = tempfile::tempdir_in(&cache)?;
    let cab_path = stage.path().join("catalogue.cab");
    std::fs::write(&cab_path, cab)?;
    let xml = command(
        Path::new("/usr/bin/tar"),
        &["-xOf", cab_path.to_str().unwrap(), "products.xml"],
    )
    .await?;
    let media = catalogue(std::str::from_utf8(&xml.stdout)?)?;
    let esd = download::fetch(&client, &media, &cache, progress).await?;
    std::fs::write(progress, "Preparing bootable Windows installer…")?;
    let files = stage.path().join("files");
    std::fs::create_dir(&files)?;
    let esd = esd.to_str().context("Invalid installer path")?;
    command(
        &wim,
        &["apply", esd, "1", files.to_str().unwrap(), "--quiet"],
    )
    .await?;
    let boot = files.join("sources/boot.wim");
    let install = files.join("sources/install.wim");
    command(
        &wim,
        &[
            "export",
            esd,
            "2",
            boot.to_str().unwrap(),
            "--compress=LZX",
            "--quiet",
        ],
    )
    .await?;
    command(
        &wim,
        &[
            "export",
            esd,
            "3",
            boot.to_str().unwrap(),
            "--compress=LZX",
            "--boot",
            "--quiet",
        ],
    )
    .await?;
    let info = command(&wim, &["info", esd]).await?;
    let count = String::from_utf8(info.stdout)?
        .lines()
        .find_map(|line| {
            line.strip_prefix("Image Count:")
                .and_then(|n| n.trim().parse::<u32>().ok())
        })
        .context("The Windows installer has no image count")?;
    if !(4..=32).contains(&count) {
        bail!("Unexpected Windows installer image count");
    }
    for index in 4..=count {
        command(
            &wim,
            &[
                "export",
                esd,
                &index.to_string(),
                install.to_str().unwrap(),
                "--compress=LZMS",
                "--quiet",
            ],
        )
        .await?;
    }
    let output = stage.path().join("installer.iso");
    command(
        &mkisofs,
        &[
            "-eltorito-platform",
            "efi",
            "-b",
            "efi/microsoft/boot/efisys.bin",
            "-no-emul-boot",
            "-udf",
            "-iso-level",
            "3",
            "-V",
            "HOPPER_WINDOWS",
            "-o",
            output.to_str().unwrap(),
            files.to_str().unwrap(),
        ],
    )
    .await?;
    std::fs::rename(output, &iso)?;
    let _ = std::fs::remove_file(esd);
    Ok(iso)
}
