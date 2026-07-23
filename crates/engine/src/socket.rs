//! The `/var/run/docker.sock` compatibility link.
//!
//! Docker contexts reach anything that reads the Docker config. They do not
//! reach the large set of tools that hardcode the well-known socket path:
//! Testcontainers, CI runners, Portainer, and every container started with
//! `-v /var/run/docker.sock:/var/run/docker.sock`. For those, the path *is*
//! the API, and a symlink is the only thing that redirects them.
//!
//! This is deliberately opt-in. The path is root-owned, pointing it at Hopper
//! affects the whole machine, and silently repointing a shared socket would be
//! a hostile thing to do to someone still running Docker Desktop.

use model::SocketCompatStatus;
use std::path::{Path, PathBuf};

pub const WELL_KNOWN: &str = "/var/run/docker.sock";

/// Where the well-known path currently points, if it is a symlink.
pub fn current_target(path: &Path) -> Option<PathBuf> {
    std::fs::read_link(path).ok()
}

/// Describe the well-known socket relative to the engine we want it to serve.
pub fn inspect(well_known: &Path, ours: &Path) -> SocketCompatStatus {
    let exists = well_known.exists() || std::fs::symlink_metadata(well_known).is_ok();
    if !exists {
        return SocketCompatStatus {
            present: false,
            ours: false,
            target: None,
            detail: format!(
                "{} does not exist. Tools that hardcode it will not find any engine.",
                well_known.display()
            ),
        };
    }

    let target = current_target(well_known);
    let points_at_us = target.as_deref() == Some(ours);
    let detail = match (&target, points_at_us) {
        (_, true) => format!("{} points at Hopper.", well_known.display()),
        (Some(t), false) => format!(
            "{} points at {} — another engine owns it.",
            well_known.display(),
            t.display()
        ),
        (None, false) => format!(
            "{} is a real socket owned by another engine, not a link.",
            well_known.display()
        ),
    };

    SocketCompatStatus {
        present: true,
        ours: points_at_us,
        target: target.map(|t| t.to_string_lossy().to_string()),
        detail,
    }
}

/// The command a user runs to point the well-known socket at Hopper.
///
/// Hopper does not execute this itself: the path is root-owned, so it needs an
/// authorization prompt, and showing the exact command is more honest than
/// asking for a privileged helper the user cannot inspect.
pub fn link_command(ours: &Path) -> String {
    format!(
        "sudo ln -sf {} {}",
        ours.display(),
        WELL_KNOWN
    )
}

/// The command that undoes it.
pub fn unlink_command() -> String {
    format!("sudo rm {WELL_KNOWN}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hoppersock{}{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_well_known_socket_is_reported_as_absent() {
        let dir = scratch("absent");
        let status = inspect(&dir.join("docker.sock"), &dir.join("ours.sock"));
        assert!(!status.present);
        assert!(!status.ours);
        assert!(status.detail.contains("does not exist"));
    }

    #[test]
    fn a_link_pointing_at_us_is_recognized() {
        let dir = scratch("ours");
        let ours = dir.join("hopper.sock");
        std::fs::write(&ours, b"").unwrap();
        let well_known = dir.join("docker.sock");
        std::os::unix::fs::symlink(&ours, &well_known).unwrap();

        let status = inspect(&well_known, &ours);
        assert!(status.present);
        assert!(status.ours);
        assert!(status.detail.contains("points at Hopper"));
    }

    #[test]
    fn a_link_owned_by_another_engine_is_reported_without_being_touched() {
        let dir = scratch("theirs");
        let theirs = dir.join("other.sock");
        std::fs::write(&theirs, b"").unwrap();
        let ours = dir.join("hopper.sock");
        let well_known = dir.join("docker.sock");
        std::os::unix::fs::symlink(&theirs, &well_known).unwrap();

        let status = inspect(&well_known, &ours);
        assert!(status.present);
        assert!(!status.ours, "another engine's link must not read as ours");
        assert!(status.detail.contains("another engine owns it"));
        // And it is still there afterwards — inspection never mutates.
        assert!(std::fs::symlink_metadata(&well_known).is_ok());
    }

    #[test]
    fn a_real_socket_rather_than_a_link_is_distinguished() {
        let dir = scratch("real");
        let well_known = dir.join("docker.sock");
        std::fs::write(&well_known, b"").unwrap();
        let status = inspect(&well_known, &dir.join("hopper.sock"));
        assert!(status.present);
        assert!(!status.ours);
        assert!(status.target.is_none());
        assert!(status.detail.contains("not a link"));
    }

    #[test]
    fn the_commands_name_both_paths_so_the_user_can_check_them() {
        let cmd = link_command(Path::new("/Users/x/.hopper/run/docker.sock"));
        assert!(cmd.contains("/Users/x/.hopper/run/docker.sock"));
        assert!(cmd.contains(WELL_KNOWN));
        assert!(cmd.starts_with("sudo ln -sf"));
        assert!(unlink_command().contains(WELL_KNOWN));
    }
}
