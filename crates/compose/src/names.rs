//! What compose calls things.
//!
//! These names are not cosmetic. Hopper finds a stack again by reading the
//! `com.docker.compose.*` labels off its containers, and so does the real
//! `docker compose` — a stack Hopper brings up appears in `docker compose ls`
//! on a machine that has it, and one Compose brought up appears in Hopper.
//! `config-hash` is what buys that: verified against Compose 5.3.1, it is the
//! label whose *presence* makes a set of containers a project rather than
//! loose containers wearing project labels.
//!
//! Compose v2 separates a container's parts with `-` and a network or volume's
//! with `_`. That looks like an inconsistency and is not one; it is the format
//! on disk today.

use std::path::Path;

pub const PROJECT: &str = "com.docker.compose.project";
pub const SERVICE: &str = "com.docker.compose.service";
pub const NUMBER: &str = "com.docker.compose.container-number";
pub const CONFIG_FILES: &str = "com.docker.compose.project.config_files";
pub const WORKING_DIR: &str = "com.docker.compose.project.working_dir";
pub const ONEOFF: &str = "com.docker.compose.oneoff";
pub const CONFIG_HASH: &str = "com.docker.compose.config-hash";
pub const DEPENDS_ON: &str = "com.docker.compose.depends_on";

/// A stable digest of whatever a service resolved to.
///
/// Compose stamps one on every container and compares it on the next `up` to
/// decide whether the container still matches the file. Hopper does the same,
/// which is what keeps a second `up` from tearing down a database that has not
/// changed.
///
/// FNV-1a rather than the standard library's hasher, whose output is explicitly
/// not stable between releases — a Rust upgrade would silently invalidate every
/// container Hopper had ever created.
pub fn config_hash(canonical: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in canonical.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Normalize a project name the way compose does: lowercase, and only
/// `[a-z0-9_-]` survives.
pub fn normalize(raw: &str) -> String {
    let cleaned: String = raw
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    cleaned.trim_matches(|c| c == '-' || c == '_').to_string()
}

/// The project name: what the caller asked for, else what the file calls
/// itself, else the directory it sits in.
pub fn project(explicit: Option<&str>, in_file: Option<&str>, dir: &Path) -> String {
    let from_dir = || {
        dir.canonicalize()
            .ok()
            .as_deref()
            .and_then(Path::file_name)
            .or_else(|| dir.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    };

    for candidate in [
        explicit.map(str::to_string),
        in_file.map(str::to_string),
        Some(from_dir()),
    ]
    .into_iter()
    .flatten()
    {
        let name = normalize(&candidate);
        if !name.is_empty() {
            return name;
        }
    }
    // A directory whose name is entirely punctuation still has to run.
    "compose".to_string()
}

/// `project-service-1`, the name compose gives a service's container.
pub fn container(project: &str, service: &str, index: u32) -> String {
    format!("{project}-{service}-{index}")
}

/// `project_name`, the name compose gives a network or a volume it creates.
pub fn scoped(project: &str, name: &str) -> String {
    format!("{project}_{name}")
}

/// `project_default`, the network every service joins when the file declares
/// none of its own.
pub fn default_network(project: &str) -> String {
    scoped(project, "default")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_name_is_lowercased_and_stripped() {
        assert_eq!(normalize("My App!"), "myapp");
        assert_eq!(normalize("shop-api"), "shop-api");
        assert_eq!(normalize("Shop_API"), "shop_api");
    }

    #[test]
    fn leading_and_trailing_separators_are_trimmed() {
        // A directory called `.hidden` would otherwise become `-hidden`.
        assert_eq!(normalize("-shop-"), "shop");
        assert_eq!(normalize("_shop_"), "shop");
    }

    #[test]
    fn an_explicit_name_beats_the_file_which_beats_the_directory() {
        let dir = Path::new("/srv/fallback");
        assert_eq!(project(Some("chosen"), Some("infile"), dir), "chosen");
        assert_eq!(project(None, Some("infile"), dir), "infile");
        assert_eq!(project(None, None, dir), "fallback");
    }

    #[test]
    fn a_name_that_normalizes_away_falls_through_to_the_next_source() {
        let dir = Path::new("/srv/realname");
        // `!!!` is not a project name, so it must not win and leave us blank.
        assert_eq!(project(Some("!!!"), None, dir), "realname");
    }

    #[test]
    fn a_project_always_ends_up_with_some_name() {
        assert_eq!(project(None, None, Path::new("/")), "compose");
    }

    #[test]
    fn a_config_hash_is_stable_and_distinguishes_different_definitions() {
        assert_eq!(config_hash("web|nginx"), config_hash("web|nginx"));
        assert_ne!(config_hash("web|nginx"), config_hash("web|caddy"));
        // Pinned: this value is written onto real containers, and changing the
        // function would silently recreate every stack anyone has running.
        assert_eq!(config_hash(""), "cbf29ce484222325");
    }

    #[test]
    fn containers_use_dashes_and_networks_use_underscores() {
        // Compose's own formats. Diverging would orphan the stack from the
        // real `docker compose`, which finds it by exactly these names.
        assert_eq!(container("shop", "web", 1), "shop-web-1");
        assert_eq!(scoped("shop", "backend"), "shop_backend");
        assert_eq!(default_network("shop"), "shop_default");
    }
}
