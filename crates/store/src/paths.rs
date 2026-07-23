//! Where Hopper keeps its state on disk.
//!
//! Everything lives under `~/.hopper/`, which the earlier TypeScript build
//! also used — the layout is deliberately unchanged so an upgrade keeps the
//! user's workspaces and settings. `HOPPER_DIR` overrides the root; tests set
//! it to a temporary directory.

use std::path::PathBuf;

pub fn root() -> PathBuf {
    if let Ok(dir) = std::env::var("HOPPER_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hopper")
}

pub fn settings_file() -> PathBuf {
    root().join("settings.json")
}

pub fn workspaces_file() -> PathBuf {
    root().join("workspaces.json")
}

/// The directory holding the managed engine's runtime sockets.
pub fn run_dir() -> PathBuf {
    root().join("run")
}

/// The socket Hopper's managed engine listens on.
pub fn engine_socket() -> PathBuf {
    run_dir().join("docker.sock")
}

/// The managed engine's VM data, including the persistent `/var/lib/docker`
/// disk.
pub fn engine_dir() -> PathBuf {
    root().join("engine")
}

pub fn log_file() -> PathBuf {
    root().join("hopper.log")
}

/// Create the root (and any parents) if it does not exist yet.
pub fn ensure_root() -> std::io::Result<PathBuf> {
    let dir = root();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_path_sits_under_the_root() {
        let root = root();
        for p in [
            settings_file(),
            workspaces_file(),
            engine_socket(),
            engine_dir(),
            log_file(),
        ] {
            assert!(p.starts_with(&root), "{p:?} escaped {root:?}");
        }
    }
}
