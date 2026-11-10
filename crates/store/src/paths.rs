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
    keep_private();
    Ok(dir)
}

/// Close the root to other accounts on this Mac.
///
/// It holds settings, engine logs, staged import archives and the engine's
/// Docker socket, and it used to be created with whatever the umask allowed.
/// Called on launch as well as on write, so a directory made by an older
/// build is tightened too. Best effort: a root on a filesystem without modes
/// is no reason to refuse to start.
pub fn keep_private() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = root();
        if dir.is_dir() {
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        for file in [settings_file(), workspaces_file()] {
            if file.is_file() {
                let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
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
