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
pub mod run;
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
pub use run::RunDialog;
pub use settings::Settings;
pub use stacks::Stacks;
pub use volumes::Volumes;

use gpui::div;
use gpui::prelude::*;
use guise::prelude::*;

use crate::state::{AppState, Route};


/// A single centred line — for empty and loading states, which must read
/// differently from each other and from a failure.
pub fn message(text: impl Into<String>) -> gpui::AnyElement {
    div()
        .p_6()
        .child(Text::new(text.into()).size(Size::Sm).dimmed())
        .into_any_element()
}

/// An empty state that teaches the next step: a heading, a hint, and a button
/// that jumps to the route where the user can act (usually the Registry).
pub fn empty_cta(
    state: &AppState,
    title: &str,
    detail: &str,
    button: &str,
    route: Route,
    _cx: &gpui::App,
) -> gpui::AnyElement {
    let go = state.route.clone();
    div()
        .p_6()
        .child(
            Stack::new()
                .gap(Size::Sm)
                .child(Text::new(title.to_string()).size(Size::Sm).medium())
                .child(Text::new(detail.to_string()).size(Size::Xs).dimmed())
                .child(
                    div().child(
                        Button::new("empty-cta", button.to_string())
                            .size(Size::Sm)
                            .variant(Variant::Light)
                            .color(ColorName::Blue)
                            .on_click(move |_, _, cx| go.set(cx, route)),
                    ),
                ),
        )
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

