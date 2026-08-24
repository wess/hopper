//! The `container` services: status, lifecycle, version, disk.
//!
//! `container system start` prompts on stdin for the default kernel when it is
//! not told what to do, and Hopper runs it with no terminal attached — so the
//! kernel-install decision is always passed explicitly. Without it the first
//! run fails with "failed to read user input" and the engine never comes up.

use docker::{DockerError, Result};
use model::{DiskUsage, SystemVersion, UsageBucket};
use serde::Deserialize;

use crate::cli::Cli;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub api_server_version: String,
    #[serde(default)]
    pub api_server_build: String,
    #[serde(default)]
    pub app_root: String,
}

impl SystemStatus {
    pub fn running(&self) -> bool {
        self.status.eq_ignore_ascii_case("running")
    }
}

/// Ask the apiserver how it is doing.
///
/// This one command reports through both channels: with the services
/// unregistered it prints `{"status":"unregistered"}` and *then* exits 1. Read
/// the body first and only fall back to "not running" when there is nothing
/// there — going by the exit code alone throws away the answer.
pub async fn status(cli: &Cli) -> Result<SystemStatus> {
    let (stdout, _ok) = cli.output(&["system", "status", "--format", "json"]).await?;
    if stdout.trim().is_empty() {
        return Ok(SystemStatus::default());
    }
    // A body we cannot read is still not a running engine.
    Ok(crate::cli::decode(&stdout).unwrap_or_default())
}

/// Start the services.
///
/// `--enable-kernel-install` lets the first run fetch the recommended kernel
/// without a prompt; there is no terminal here to answer one.
pub async fn start(cli: &Cli) -> Result<()> {
    cli.ok(&["system", "start", "--enable-kernel-install"]).await
}

pub async fn stop(cli: &Cli) -> Result<()> {
    cli.ok(&["system", "stop"]).await
}

/// The CLI's own version, e.g. `container CLI version 1.2.2`.
pub async fn version(cli: &Cli) -> Result<SystemVersion> {
    let raw = cli.run(&["--version"]).await?;
    let version = parse_version(&raw);
    let status = status(cli).await.unwrap_or_default();
    Ok(SystemVersion {
        version,
        api_version: status.api_server_version,
        os: "macos".into(),
        arch: std::env::consts::ARCH.into(),
        kernel_version: String::new(),
        go_version: String::new(),
        build_time: status.api_server_build,
    })
}

/// Pull the version number out of whatever `--version` printed.
///
/// Apple has changed the wording between releases, so take the first
/// dotted-numeric token rather than trusting a prefix.
pub fn parse_version(raw: &str) -> String {
    raw.split_whitespace()
        .map(|t| t.trim_start_matches('v'))
        .find(|t| {
            let mut parts = t.split('.');
            parts.clone().count() >= 2
                && parts.all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        })
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DfEntry {
    #[serde(default)]
    count: i64,
    #[serde(default)]
    size: i64,
    #[serde(default)]
    reclaimable: i64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Df {
    #[serde(default)]
    images: DfEntry,
    #[serde(default)]
    containers: DfEntry,
    #[serde(default)]
    volumes: DfEntry,
    #[serde(default)]
    builder: DfEntry,
}

pub async fn df(cli: &Cli) -> Result<DiskUsage> {
    let raw = cli.run(&["system", "df", "--format", "json"]).await?;
    let d: Df = crate::cli::decode(&raw)?;
    let bucket = |e: DfEntry| UsageBucket {
        count: e.count,
        size: e.size,
        reclaimable: e.reclaimable,
    };
    Ok(DiskUsage {
        images: bucket(d.images),
        containers: bucket(d.containers),
        volumes: bucket(d.volumes),
        build_cache: bucket(d.builder),
    })
}

/// A one-line reason the runtime cannot be used here, or `None` when it can.
pub fn unsupported_reason() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some("Apple Containers only runs on macOS.".into());
    }
    None
}

/// Confirm the machine is new enough. Apple requires macOS 26 for the vmnet
/// APIs that container-to-container networking depends on.
///
/// Read once and kept: the OS cannot change under a running process, and the
/// engine poll would otherwise fork `sw_vers` several times a tick forever.
pub fn macos_major() -> Option<u32> {
    static MAJOR: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *MAJOR.get_or_init(|| {
        let out = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.trim().split('.').next()?.parse().ok()
    })
}

pub fn too_old() -> Option<DockerError> {
    match macos_major() {
        Some(major) if major < 26 => Some(DockerError::transport(format!(
            "Apple Containers needs macOS 26 or later; this Mac is on {major}."
        ))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_number_survives_apples_wording() {
        assert_eq!(parse_version("container CLI version 1.2.2"), "1.2.2");
        assert_eq!(parse_version("1.2.2"), "1.2.2");
        assert_eq!(parse_version("container version v1.2.2 (build abc)"), "1.2.2");
    }

    #[test]
    fn a_build_hash_is_not_mistaken_for_a_version() {
        // A commit like `a8395a1` has no dots; `1.2.2` does.
        assert_eq!(parse_version("container a8395a1 1.2.2"), "1.2.2");
    }

    #[test]
    fn unrecognisable_output_yields_an_empty_version_rather_than_a_panic() {
        assert_eq!(parse_version(""), "");
        assert_eq!(parse_version("no numbers here"), "");
    }

    #[test]
    fn status_reads_running_case_insensitively() {
        let s = SystemStatus { status: "Running".into(), ..Default::default() };
        assert!(s.running());
        assert!(!SystemStatus::default().running());
    }

    #[test]
    fn unregistered_services_are_not_running() {
        // The literal value `container system status` prints on a machine
        // where the package is present but was never started.
        let s = SystemStatus { status: "unregistered".into(), ..Default::default() };
        assert!(!s.running());
    }
}
