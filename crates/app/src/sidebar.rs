//! The navigation rail plus the engine-status footer.

use gpui::prelude::*;
use gpui::{div, SharedString};
use guise::prelude::*;

use crate::state::{AppState, Route};
use crate::theme;
use model::EngineState;

fn engine_accent(state: EngineState) -> ColorName {
    match state {
        EngineState::Connected => ColorName::Green,
        EngineState::Starting => ColorName::Blue,
        EngineState::Stopped | EngineState::NotInstalled => ColorName::Gray,
        EngineState::NeedsPermission | EngineState::Unsupported => ColorName::Orange,
        EngineState::Unreachable => ColorName::Red,
    }
}

pub fn render(state: &AppState, cx: &mut gpui::App) -> impl IntoElement {
    let palette = theme::palette(cx);
    let active = state.route.get(cx);
    let engine = state.engine.get(cx);

    let mut nav = Stack::new().gap(Size::Xs);
    for route in Route::all() {
        let selected = route == active;
        let for_click = route;
        let signal = state.route.clone();
        nav = nav.child(
            Button::new(
                SharedString::from(format!("nav-{}", route.label())),
                route.label(),
            )
            .full_width(true)
            .variant(if selected {
                Variant::Light
            } else {
                Variant::Subtle
            })
            .color(if selected {
                ColorName::Blue
            } else {
                ColorName::Gray
            })
            .size(Size::Sm)
            .on_click(move |_, _, cx| signal.set(cx, for_click)),
        );
    }

    let footer = Stack::new()
        .gap(Size::Xs)
        .child(
            Group::new()
                .gap(Size::Xs)
                .align(Align::Center)
                .child(
                    Badge::new(engine.state.as_str())
                        .variant(Variant::Light)
                        .color(engine_accent(engine.state))
                        .size(Size::Xs),
                ),
        )
        .child(Text::new(engine.message.clone()).size(Size::Xs).dimmed());

    div()
        .flex()
        .flex_col()
        .justify_between()
        .w(gpui::px(212.0))
        .h_full()
        .p_3()
        .bg(palette.bg_subtle)
        .border_r_1()
        .border_color(palette.border_subtle)
        .child(
            Stack::new()
                .gap(Size::Sm)
                .child(Text::new("Hopper").size(Size::Lg).bold())
                .child(nav),
        )
        .child(footer)
}
