//! Settings: engine control, resources, file sharing, and CLI integration.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::AppState;
use crate::theme;
use model::EngineState;

pub struct Settings {
    state: AppState,
    busy: bool,
    notice: Option<String>,
}

impl Settings {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.engine);
        watch(cx, &state.settings);
        Self {
            state,
            busy: false,
            notice: None,
        }
    }

    fn start_engine(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host.start_engine().await.map_err(|e| e.to_string()) },
            move |result, cx| {
                if let Err(e) = result {
                    tracing::warn!("engine start failed: {e}");
                }
                state.bump(cx);
            },
        );
    }

    fn stop_engine(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = true;
        cx.notify();
        bridge::run(
            cx,
            async move { host.stop_engine().await.map_err(|e| e.to_string()) },
            move |_, cx| state.bump(cx),
        );
    }

    fn section(&self, title: &str, body: impl IntoElement, cx: &gpui::App) -> impl IntoElement {
        let palette = theme::palette(cx);
        div()
            .p_4()
            .rounded_md()
            .bg(palette.bg_subtle)
            .border_1()
            .border_color(palette.border_subtle)
            .child(
                Stack::new()
                    .gap(Size::Sm)
                    .child(Text::new(title.to_string()).size(Size::Sm).bold())
                    .child(body),
            )
    }
}

impl Render for Settings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let engine = self.state.engine.get(cx);
        let settings = self.state.settings.get(cx);

        let can_start = matches!(
            engine.state,
            EngineState::Stopped | EngineState::NotInstalled | EngineState::Unreachable
        ) && engine.managed;
        let can_stop = engine.connected && engine.managed;

        let engine_body = Stack::new()
            .gap(Size::Sm)
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .align(Align::Center)
                    .child(
                        Badge::new(engine.state.as_str())
                            .variant(Variant::Light)
                            .color(if engine.connected {
                                ColorName::Green
                            } else {
                                ColorName::Gray
                            })
                            .size(Size::Xs),
                    )
                    .child(Text::new(engine.provider.clone()).size(Size::Xs).dimmed()),
            )
            .child(Text::new(engine.message.clone()).size(Size::Xs))
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .child(
                        Button::new("engine-start", "Start engine")
                            .size(Size::Xs)
                            .variant(Variant::Light)
                            .color(ColorName::Green)
                            // Only a managed engine can be started from here;
                            // an engine someone else runs is theirs to control.
                            .disabled(self.busy || !can_start)
                            .on_click(cx.listener(|this, _, _, cx| this.start_engine(cx))),
                    )
                    .child(
                        Button::new("engine-stop", "Stop engine")
                            .size(Size::Xs)
                            .variant(Variant::Subtle)
                            .color(ColorName::Red)
                            .disabled(self.busy || !can_stop)
                            .on_click(cx.listener(|this, _, _, cx| this.stop_engine(cx))),
                    ),
            );

        let resources = &settings.resources;
        let resources_body = Stack::new()
            .gap(Size::Xs)
            .child(
                Text::new(format!(
                    "{} CPUs · {} GiB memory · {} GiB disk",
                    resources.cpus, resources.memory_gib, resources.disk_gib
                ))
                .size(Size::Xs),
            )
            .child(
                Text::new("CPU and memory apply when the engine restarts.")
                    .size(Size::Xs)
                    .dimmed(),
            );

        let shares = if settings.shared_paths.is_empty() {
            "Your home directory only.".to_string()
        } else {
            format!(
                "Home directory, plus: {}",
                settings.shared_paths.join(", ")
            )
        };
        let sharing_body = Stack::new()
            .gap(Size::Xs)
            .child(Text::new(shares).size(Size::Xs))
            .child(
                Text::new(
                    "A bind mount outside these paths shows the container an empty \
                     directory rather than your files.",
                )
                .size(Size::Xs)
                .dimmed(),
            );

        let cli_body = Stack::new()
            .gap(Size::Xs)
            .child(
                Text::new(format!(
                    "export DOCKER_HOST=\"{}\"",
                    engine.endpoint.clone().unwrap_or_default()
                ))
                .size(Size::Xs),
            )
            .child(
                Text::new(
                    "Or create a `hopper` Docker context so `docker` and `docker compose` \
                     target Hopper.",
                )
                .size(Size::Xs)
                .dimmed(),
            );

        let theme_body = Text::new(format!("Theme: {}", settings.theme.as_str())).size(Size::Xs);

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .items_center()
                    .p_4()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(Text::new("Settings").size(Size::Xl).bold()),
            )
            .child(
                div().flex_1().overflow_hidden().p_4().child(
                    Stack::new()
                        .gap(Size::Md)
                        .child(self.section("Engine", engine_body, cx))
                        .child(self.section("Resources", resources_body, cx))
                        .child(self.section("File sharing", sharing_body, cx))
                        .child(self.section("Docker CLI", cli_body, cx))
                        .child(self.section("Appearance", theme_body, cx)),
                ),
            )
    }
}
