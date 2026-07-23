//! One module per view.

pub mod ansi;
pub mod containers;
pub mod dashboard;
pub mod detail;
pub mod engine;
pub mod files;
pub mod terminal;
pub mod images;
pub mod networks;
pub mod registry;
pub mod settings;
pub mod stacks;
pub mod volumes;

pub use containers::Containers;
pub use dashboard::Dashboard;
pub use detail::Detail;
pub use engine::EngineSetup;
pub use images::Images;
pub use networks::Networks;
pub use registry::Registry;
pub use settings::Settings;
pub use stacks::Stacks;
pub use volumes::Volumes;

use gpui::div;
use gpui::prelude::*;
use guise::prelude::*;


/// A single centred line — for empty and loading states, which must read
/// differently from each other and from a failure.
pub fn message(text: impl Into<String>) -> gpui::AnyElement {
    div()
        .p_6()
        .child(Text::new(text.into()).size(Size::Sm).dimmed())
        .into_any_element()
}

/// A failure, showing the daemon's own words underneath a plain summary.
pub fn failure(summary: &str, detail: &str) -> gpui::AnyElement {
    div()
        .p_6()
        .child(
            Stack::new()
                .gap(Size::Xs)
                .child(Text::new(summary.to_string()).size(Size::Sm).medium())
                .child(Text::new(detail.to_string()).size(Size::Xs).dimmed()),
        )
        .into_any_element()
}

