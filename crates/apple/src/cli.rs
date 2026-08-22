//! Finding and driving the `container` binary.
//!
//! Apple's runtime has no Engine API and no socket: the CLI talks to
//! `container-apiserver` over XPC, and asking for a Docker-compatible endpoint
//! was closed as not planned. So this is the transport — a process per call,
//! `--format json` where the command offers it.
//!
//! Every list and inspect command renders through Apple's `Output.renderJSON`,
//! which encodes dates as ISO8601 rather than Swift's reference-epoch default.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use docker::{DockerError, Result};
use serde::de::DeserializeOwned;
use tokio::process::Command;

/// Where the signed installer puts things. The package declares
/// `install-location=/usr/local`, so the binary is fixed unless someone moved it.
const INSTALLED: &str = "/usr/local/bin/container";

/// Overrides the binary path, for tests and unusual installs.
const BIN_ENV: &str = "HOPPER_CONTAINER_BIN";

#[derive(Clone, Debug)]
pub struct Cli {
    bin: PathBuf,
}

impl Cli {
    /// Wrap a known binary path without probing for it.
    pub fn at(bin: impl Into<PathBuf>) -> Self {
        Self { bin: bin.into() }
    }

    /// Find `container`, or report that this machine has none.
    ///
    /// Checked in the order a user would expect to win: an explicit override,
    /// the installed location, then anything on `PATH`.
    pub fn locate() -> Option<Self> {
        if let Some(p) = std::env::var_os(BIN_ENV) {
            let p = PathBuf::from(p);
            return executable(&p).then_some(Self { bin: p });
        }
        let installed = Path::new(INSTALLED);
        if executable(installed) {
            return Some(Self {
                bin: installed.to_path_buf(),
            });
        }
        which("container").map(|bin| Self { bin })
    }

    pub fn path(&self) -> &Path {
        &self.bin
    }

    /// Run a command, returning stdout on success.
    pub async fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| {
                DockerError::transport(format!("could not run `{}`: {e}", self.bin.display()))
            })?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err(classify(
            &String::from_utf8_lossy(&output.stderr),
            args,
            output.status.code(),
        ))
    }

    /// Run a command and discard its output.
    pub async fn ok(&self, args: &[&str]) -> Result<()> {
        self.run(args).await.map(|_| ())
    }

    /// Run a command with `--format json` and decode the payload.
    pub async fn json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T> {
        let mut argv = args.to_vec();
        argv.extend_from_slice(&["--format", "json"]);
        let raw = self.run(&argv).await?;
        decode(&raw)
    }
}

/// Decode a `--format json` payload.
///
/// Empty output is a valid empty list: `container ls` on a machine with no
/// containers prints nothing at all rather than `[]`.
pub fn decode<T: DeserializeOwned>(raw: &str) -> Result<T> {
    let trimmed = raw.trim();
    let body = if trimmed.is_empty() { "[]" } else { trimmed };
    serde_json::from_str(body).map_err(|e| {
        DockerError::decode(format!(
            "could not read the output of `container`: {e}"
        ))
    })
}

/// Turn a failed run into an error the rest of Hopper already knows how to
/// handle.
///
/// The status codes are Docker's, not Apple's — `is_not_found` and
/// `is_conflict` are what the views branch on, so a missing container has to
/// read as 404 whichever backend reported it.
fn classify(stderr: &str, args: &[&str], code: Option<i32>) -> DockerError {
    let message = tidy(stderr, args);
    let lower = message.to_lowercase();

    if lower.contains("not found") || lower.contains("does not exist") {
        return DockerError::api(404, message);
    }
    if lower.contains("already exists") || lower.contains("already in use") || lower.contains("in use by") {
        return DockerError::api(409, message);
    }
    if lower.contains("permission denied") || lower.contains("not permitted") {
        return DockerError::permission(message);
    }
    // The apiserver is registered with launchd but not running, or was never
    // started. Transport, so the engine reads as stopped rather than broken.
    if lower.contains("connection refused")
        || lower.contains("could not connect")
        || lower.contains("xpc")
        || lower.contains("is not running")
    {
        return DockerError::transport(message);
    }
    DockerError::api(code.unwrap_or(1).clamp(0, 599) as u16, message)
}

/// Apple prefixes its diagnostics with `Error:` and sometimes repeats the
/// usage block. Keep the first meaningful line, and say which command failed
/// when there is nothing else to go on.
fn tidy(stderr: &str, args: &[&str]) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("Usage:") && !l.starts_with("OPTIONS"))
        .unwrap_or("");
    let line = line
        .trim_start_matches("Error:")
        .trim_start_matches("error:")
        .trim();
    if line.is_empty() {
        format!("`container {}` failed", args.join(" "))
    } else {
        line.to_string()
    }
}

fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// A `PATH` walk, so a Homebrew or hand-placed `container` is still found.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|c| executable(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_output_reads_as_an_empty_list() {
        // `container ls` prints nothing when there is nothing to list, which
        // is not valid JSON and must not surface as a decode failure.
        let v: Vec<String> = decode("").unwrap();
        assert!(v.is_empty());
        let v: Vec<String> = decode("   \n ").unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn a_missing_object_reads_as_not_found() {
        let e = classify("Error: container not found: web", &["inspect"], Some(1));
        assert!(e.is_not_found(), "views branch on 404 regardless of backend");
        assert_eq!(e.message, "container not found: web");
    }

    #[test]
    fn a_name_clash_reads_as_a_conflict() {
        let e = classify("Error: volume already exists", &["volume", "create"], Some(1));
        assert!(e.is_conflict());
    }

    #[test]
    fn an_unreachable_apiserver_reads_as_transport_not_api() {
        // Transport is what makes the engine report "stopped" (and therefore
        // startable) instead of "the daemon said something odd".
        let e = classify("Error: could not connect to apiserver", &["ls"], Some(1));
        assert_eq!(e.kind, docker::ErrorKind::Transport);
    }

    #[test]
    fn a_bare_failure_still_names_the_command() {
        let e = classify("", &["volume", "ls"], Some(2));
        assert!(e.message.contains("container volume ls"));
    }

    #[test]
    fn usage_noise_is_skipped_in_favour_of_the_real_line() {
        let e = classify(
            "Usage: container run <image>\nError: image not found\n",
            &["run"],
            Some(1),
        );
        assert_eq!(e.message, "image not found");
    }

    #[test]
    fn an_override_that_points_nowhere_finds_nothing() {
        // Guards the first-run path: a stale override must report "not
        // installed" rather than silently falling through to some other binary.
        std::env::set_var(BIN_ENV, "/nonexistent/container");
        assert!(Cli::locate().is_none());
        std::env::remove_var(BIN_ENV);
    }
}
