//! Hopper's Guise updater configuration.

use gpui::App;
use guise::update::Updater;

const TEAM: &str = "XJDC46F35X";

fn updater() -> Updater {
    Updater::github("Hopper", env!("CARGO_PKG_VERSION"), "wess/hopper")
        .codesign_requirement(format!(
            "anchor apple generic and certificate leaf[subject.OU] = {TEAM}"
        ))
        .require_checksum(true)
        .window_title("Hopper Update")
}

/// Check at launch and then at Guise's bounded hourly cadence.
pub fn start(cx: &mut App) {
    guise::update::start(updater(), cx);
}

/// Answer a user-requested check even when there is nothing to install.
pub fn check_now(cx: &mut App) {
    guise::update::check_now(updater(), cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_are_checked_with_strict_artifact_verification() {
        let updater = updater();
        assert_eq!(updater.app(), "Hopper");
        assert_eq!(updater.version(), env!("CARGO_PKG_VERSION"));
        assert!(updater.config().requires_checksum());
        assert_eq!(
            updater.config().source().endpoint(),
            "https://api.github.com/repos/wess/hopper/releases/latest"
        );
    }
}
