//! Exposing the guest's Docker socket on the host.
//!
//! `dockerd` inside the guest is reachable only over vsock. Everything on the
//! host — Hopper itself, the `docker` CLI, Compose, Testcontainers — speaks to
//! a unix socket. This listens at `~/.hopper/run/docker.sock` and splices each
//! accepted connection through to the guest.
//!
//! This is the piece that makes the managed engine reachable at all.

use std::path::{Path, PathBuf};
use tokio::net::UnixListener;

/// Remove a socket left behind by a previous run.
///
/// A crash leaves the file in place, and `bind` then fails with "address in
/// use" even though nothing is listening. Only stale *sockets* are removed —
/// deleting anything else would be destroying a file we do not own.
pub fn clear_stale(path: &Path) -> std::io::Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(()); // nothing there
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        if !meta.file_type().is_socket() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} exists and is not a socket; refusing to remove it",
                    path.display()
                ),
            ));
        }
    }
    let _ = meta;
    std::fs::remove_file(path)
}

/// Prepare the socket path: make the directory, clear a stale socket.
pub fn prepare(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    clear_stale(path)
}

/// Bind the listener the host talks to.
pub fn listen(path: &PathBuf) -> std::io::Result<UnixListener> {
    prepare(path)?;
    let listener = UnixListener::bind(path)?;
    // The socket is the whole engine: anyone who can open it controls Docker,
    // which is root-equivalent. Keep it to the owner.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(listener)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use crate::vz::machine::Machine;
    use crate::vz::vm::DOCKER_VSOCK_PORT;
    use std::sync::Arc;

    /// Serve the guest's Docker socket at `path` until the future is dropped.
    pub async fn serve(machine: Arc<Machine>, path: PathBuf) -> anyhow::Result<()> {
        let listener = listen(&path)?;
        tracing::info!("engine socket listening at {}", path.display());

        loop {
            let (mut host_side, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::warn!("engine socket accept failed: {e}");
                    continue;
                }
            };
            let machine = Arc::clone(&machine);
            // One task per connection: a slow or stuck client must not block
            // the next `docker ps`.
            tokio::spawn(async move {
                match machine.connect(DOCKER_VSOCK_PORT).await {
                    Ok(mut guest_side) => {
                        let _ =
                            tokio::io::copy_bidirectional(&mut host_side, &mut guest_side).await;
                    }
                    Err(e) => tracing::warn!("could not reach dockerd in the guest: {e}"),
                }
            });
        }
    }
}

#[cfg(target_os = "macos")]
pub use platform::serve;

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hopperbridge{}{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn clearing_a_path_that_does_not_exist_is_fine() {
        let path = scratch("absent").join("docker.sock");
        assert!(clear_stale(&path).is_ok());
    }

    #[tokio::test]
    async fn a_stale_socket_is_removed_so_bind_succeeds() {
        let path = scratch("stale").join("docker.sock");
        // Leave a socket behind the way a crash would.
        let first = UnixListener::bind(&path).unwrap();
        drop(first);
        assert!(path.exists(), "the file outlives the listener");

        // Binding again without clearing would fail with "address in use".
        let listener = listen(&path);
        assert!(listener.is_ok(), "{:?}", listener.err());
    }

    #[test]
    fn a_regular_file_at_the_socket_path_is_not_deleted() {
        // Something else owns this path; destroying it would be data loss.
        let dir = scratch("regular");
        let path = dir.join("docker.sock");
        std::fs::write(&path, b"important").unwrap();

        let err = clear_stale(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"important");
    }

    #[tokio::test]
    async fn preparing_creates_the_run_directory() {
        let path = scratch("mkdir").join("nested").join("run").join("docker.sock");
        assert!(prepare(&path).is_ok());
        assert!(path.parent().unwrap().is_dir());
    }

    #[tokio::test]
    async fn the_socket_is_owner_only_because_it_is_root_equivalent() {
        use std::os::unix::fs::PermissionsExt;
        let path = scratch("perms").join("docker.sock");
        let _listener = listen(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "anyone who can open this socket controls Docker"
        );
    }
}
