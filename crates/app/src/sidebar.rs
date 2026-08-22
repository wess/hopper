//! The navigation rail: Lucide-iconed routes, a collapse toggle, and the
//! engine-status footer. Collapses to an icon-only rail.

use gpui::prelude::*;
use gpui::{div, px, SharedString};
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
    let collapsed = state.sidebar_collapsed.get(cx);

    // Resolved once for the custom (left-aligned) expanded rows.
    let t = guise::theme::theme(cx);
    let blue = t.color(ColorName::Blue, t.primary_shade()).hsla();
    let label_color = t.text().hsla();
    let selected_bg = gpui::hsla(blue.h, blue.s, blue.l, 0.15);
    let hover_bg = palette.bg_muted;

    // One nav entry. Expanded: a left-aligned icon + label row; collapsed: an
    // icon-only action. Both drive the same route signal.
    let mut nav = Stack::new().gap(Size::Xs);
    for route in Route::available(&state.host.capabilities()) {
        let selected = route == active;
        let signal = state.route.clone();
        let go = route;

        let entry = if collapsed {
            let accent = if selected { ColorName::Blue } else { ColorName::Gray };
            let variant = if selected { Variant::Light } else { Variant::Subtle };
            div().flex().justify_center().child(
                ActionIcon::new(
                    SharedString::from(format!("nav-{}", route.label())),
                    route.icon(),
                )
                .variant(variant)
                .color(accent)
                .size(Size::Md)
                .on_click(move |_, _, cx| signal.set(cx, go)),
            )
            .into_any_element()
        } else {
            // A nav row reads better left-aligned than a centered button.
            let fg = if selected { blue } else { label_color };
            div()
                .id(SharedString::from(format!("nav-{}", route.label())))
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .px_2()
                .py(px(7.0))
                .rounded_md()
                .cursor_pointer()
                .text_color(fg)
                .when(selected, |d| d.bg(selected_bg))
                .when(!selected, |d| d.hover(move |st| st.bg(hover_bg)))
                .child(Icon::new(route.icon()).size(Size::Sm))
                .child(Text::new(route.label()).size(Size::Sm))
                .on_click(move |_, _, cx| signal.set(cx, go))
                .into_any_element()
        };
        nav = nav.child(entry);
    }

    // The collapse toggle. PanelLeftClose when open, PanelLeftOpen when shut.
    let toggle_signal = state.sidebar_collapsed.clone();
    let toggle = ActionIcon::new(
        "sidebar-toggle",
        if collapsed {
            IconName::PanelLeftOpen
        } else {
            IconName::PanelLeftClose
        },
    )
    .variant(Variant::Subtle)
    .color(ColorName::Gray)
    .size(Size::Sm)
    .on_click(move |_, _, cx| toggle_signal.update(cx, |c| *c = !*c));

    let header = if collapsed {
        div().flex().justify_center().child(toggle)
    } else {
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(Text::new("Hopper").size(Size::Lg).bold())
            .child(toggle)
    };

    // Footer: the engine badge + message expanded, a status dot collapsed.
    let footer = if collapsed {
        let t = guise::theme::theme(cx);
        let dot = t.color(engine_accent(engine.state), 4).hsla();
        div().flex().justify_center().child(
            div()
                .w(px(9.0))
                .h(px(9.0))
                .rounded_full()
                .bg(dot),
        )
    } else {
        div().child(
            Stack::new()
                .gap(Size::Xs)
                .child(
                    Badge::new(engine.state.as_str())
                        .variant(Variant::Light)
                        .color(engine_accent(engine.state))
                        .size(Size::Xs),
                )
                .child(Text::new(engine.message.clone()).size(Size::Xs).dimmed()),
        )
    };

    div()
        .flex()
        .flex_col()
        .justify_between()
        .w(px(if collapsed { 60.0 } else { 212.0 }))
        .flex_none()
        .h_full()
        .p_3()
        .bg(palette.bg_subtle)
        .border_r_1()
        .border_color(palette.border_subtle)
        .child(Stack::new().gap(Size::Sm).child(header).child(nav))
        .child(footer)
}
