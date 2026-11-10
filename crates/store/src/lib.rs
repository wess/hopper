//! Hopper's local persistence: JSON documents under `~/.hopper/` plus the OS
//! keychain for secrets. gpui-free.

pub mod json;
pub mod keychain;
pub mod paths;

use model::{Settings, Workspace};

/// Load the user's settings, falling back to defaults.
pub fn load_settings() -> Settings {
    json::read_or_default(&paths::settings_file())
}

pub fn save_settings(settings: &Settings) -> std::io::Result<()> {
    json::write(&paths::settings_file(), settings)
}

/// The saved workspaces. The built-in "all" scope is not stored — it is the
/// absence of a selection.
pub fn load_workspaces() -> Vec<Workspace> {
    json::read_or_default(&paths::workspaces_file())
}

pub fn save_workspaces(workspaces: &[Workspace]) -> std::io::Result<()> {
    json::write(&paths::workspaces_file(), &workspaces)
}

/// Add or replace a workspace by id.
pub fn upsert_workspace(mut all: Vec<Workspace>, ws: Workspace) -> Vec<Workspace> {
    match all.iter_mut().find(|w| w.id == ws.id) {
        Some(slot) => *slot = ws,
        None => all.push(ws),
    }
    all
}

pub fn remove_workspace(all: Vec<Workspace>, id: &str) -> Vec<Workspace> {
    all.into_iter().filter(|w| w.id != id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(id: &str, name: &str) -> Workspace {
        Workspace {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn upsert_appends_a_new_workspace() {
        let all = upsert_workspace(vec![], ws("a", "A"));
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "A");
    }

    #[test]
    fn upsert_replaces_in_place_rather_than_duplicating() {
        let all = upsert_workspace(vec![ws("a", "A"), ws("b", "B")], ws("a", "Renamed"));
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "Renamed");
        // Order is preserved so the sidebar does not reshuffle on edit.
        assert_eq!(all[1].id, "b");
    }

    #[test]
    fn removing_an_unknown_id_is_a_no_op() {
        let all = remove_workspace(vec![ws("a", "A")], "zzz");
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn removing_drops_only_the_named_workspace() {
        let all = remove_workspace(vec![ws("a", "A"), ws("b", "B")], "a");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "b");
    }
}
