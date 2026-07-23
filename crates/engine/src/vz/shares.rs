//! Host directories shared into the guest.
//!
//! The Swift `hopperd` shared exactly one directory — the user's home — and
//! nothing else. A bind mount outside it did not fail: Docker created an empty
//! directory in the guest and the container saw nothing. That is a
//! silent-data-loss-shaped failure, and it hits `/opt`, `/tmp`, `/usr/local`,
//! `/Volumes/*` (external drives), and every sibling user directory.
//!
//! Docker Desktop solves it with an editable list under Settings → Resources →
//! File sharing. This is that list, plus the check that turns a silently empty
//! mount into a warning the user can act on.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// One directory exposed to the guest.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Share {
    pub path: PathBuf,
    pub read_only: bool,
}

impl Share {
    /// The virtiofs tag. The guest mounts each share at its real host path so
    /// `-v /opt/data:/data` resolves to the same bytes it would natively.
    pub fn tag(&self) -> String {
        // Tags are limited in length and character set, so a stable hash of
        // the path is safer than the path itself.
        let mut hash: u64 = 1469598103934665603;
        for byte in self.path.to_string_lossy().as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
        format!("hs{hash:x}")
    }
}

/// The home directory always shared, matching the previous behavior so an
/// upgrade does not break existing mounts.
pub fn default_shares() -> Vec<PathBuf> {
    dirs::home_dir().into_iter().collect()
}

/// Whether `inner` sits inside `outer`.
fn is_within(inner: &Path, outer: &Path) -> bool {
    inner.starts_with(outer)
}

/// Resolve the configured paths into the shares to attach.
///
/// Nested entries are dropped: sharing both `/Users/x` and `/Users/x/code`
/// would expose the same files twice and the guest mount points would collide.
/// Paths that do not exist are dropped too — attaching one fails VM startup,
/// and a stale entry in settings must not make the engine unbootable.
pub fn resolve(configured: &[String], exists: impl Fn(&Path) -> bool) -> Vec<Share> {
    let mut candidates: BTreeSet<PathBuf> = default_shares().into_iter().collect();
    for raw in configured {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        candidates.insert(PathBuf::from(trimmed));
    }

    let present: Vec<PathBuf> = candidates.into_iter().filter(|p| exists(p)).collect();

    present
        .iter()
        .filter(|p| {
            // Keep a path only when no *other* kept path contains it.
            !present
                .iter()
                .any(|other| other != *p && is_within(p, other))
        })
        .map(|p| Share {
            path: p.clone(),
            read_only: false,
        })
        .collect()
}

/// Host paths a container wants to bind-mount that no share covers.
///
/// These are the mounts that would silently resolve to an empty directory, so
/// the UI can say so before the user spends an hour wondering why their code
/// is not in the container.
pub fn unshared_binds(host_paths: &[String], shares: &[Share]) -> Vec<String> {
    host_paths
        .iter()
        .filter(|raw| {
            let path = Path::new(raw.trim());
            // Only absolute host paths are bind mounts; a bare name is a
            // named volume and lives inside the guest.
            path.is_absolute()
                && !shares.iter().any(|s| is_within(path, &s.path))
        })
        .cloned()
        .collect()
}

/// The warning shown when a bind mount is not covered by any share.
pub fn unshared_warning(paths: &[String]) -> String {
    let list = paths.join(", ");
    format!(
        "{list} is not shared with Hopper's engine, so the container will see an \
         empty directory instead of your files. Add it under Settings → Resources → \
         File sharing."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn always(_: &Path) -> bool {
        true
    }

    fn only(paths: Vec<PathBuf>) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.iter().any(|w| w == p)
    }

    #[test]
    fn the_home_directory_is_shared_by_default() {
        let shares = resolve(&[], always);
        let home = dirs::home_dir().unwrap();
        assert!(shares.iter().any(|s| s.path == home));
    }

    #[test]
    fn a_configured_path_outside_home_is_shared() {
        // The whole point: /opt/data must reach the container.
        let shares = resolve(&["/opt/data".into()], always);
        assert!(shares.iter().any(|s| s.path == PathBuf::from("/opt/data")));
    }

    #[test]
    fn a_path_nested_inside_another_share_is_dropped() {
        let home = dirs::home_dir().unwrap();
        let nested = home.join("code");
        let shares = resolve(&[nested.to_string_lossy().to_string()], always);
        assert!(
            !shares.iter().any(|s| s.path == nested),
            "sharing a directory already covered would collide in the guest"
        );
        assert!(shares.iter().any(|s| s.path == home));
    }

    #[test]
    fn a_path_that_does_not_exist_is_dropped_rather_than_failing_startup() {
        let shares = resolve(
            &["/definitely/not/here".into()],
            only(vec![dirs::home_dir().unwrap()]),
        );
        assert!(!shares
            .iter()
            .any(|s| s.path == PathBuf::from("/definitely/not/here")));
    }

    #[test]
    fn blank_entries_are_ignored() {
        let shares = resolve(&["".into(), "   ".into()], always);
        assert_eq!(shares.len(), 1, "only the default home share should remain");
    }

    #[test]
    fn duplicate_entries_collapse() {
        let shares = resolve(&["/opt/data".into(), "/opt/data".into()], always);
        assert_eq!(
            shares
                .iter()
                .filter(|s| s.path == PathBuf::from("/opt/data"))
                .count(),
            1
        );
    }

    #[test]
    fn tags_are_stable_and_distinct_per_path() {
        let a = Share {
            path: PathBuf::from("/opt/data"),
            read_only: false,
        };
        let b = Share {
            path: PathBuf::from("/opt/other"),
            read_only: false,
        };
        assert_eq!(a.tag(), a.tag(), "a tag must not change between boots");
        assert_ne!(a.tag(), b.tag());
        // Tags must be safe as virtiofs identifiers.
        assert!(a.tag().chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn a_bind_mount_inside_a_share_is_not_flagged() {
        let shares = vec![Share {
            path: PathBuf::from("/Users/x"),
            read_only: false,
        }];
        assert!(unshared_binds(&["/Users/x/code".into()], &shares).is_empty());
    }

    #[test]
    fn a_bind_mount_outside_every_share_is_flagged() {
        let shares = vec![Share {
            path: PathBuf::from("/Users/x"),
            read_only: false,
        }];
        let flagged = unshared_binds(&["/opt/data".into()], &shares);
        assert_eq!(flagged, vec!["/opt/data".to_string()]);
    }

    #[test]
    fn named_volumes_are_not_mistaken_for_unshared_binds() {
        // `-v pgdata:/var/lib/postgresql` lives inside the guest and needs no
        // share; flagging it would be a false alarm on most compose files.
        let shares = vec![Share {
            path: PathBuf::from("/Users/x"),
            read_only: false,
        }];
        assert!(unshared_binds(&["pgdata".into()], &shares).is_empty());
    }

    #[test]
    fn the_warning_names_the_path_and_where_to_fix_it() {
        let warning = unshared_warning(&["/opt/data".into()]);
        assert!(warning.contains("/opt/data"));
        assert!(warning.contains("empty directory"));
        assert!(warning.contains("File sharing"));
    }
}
