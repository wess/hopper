//! Settings: engine choice and control, CLI integration, and appearance.
//!
//! No resource budget and no share list: Apple sizes each container's VM when
//! it runs it, and bind-mounts host paths directly. Both sections described
//! the VM Hopper used to run, and showing numbers that change nothing is worse
//! than not showing them.
//!
//! The engine picker is the one place a person moving off Docker Desktop can
//! stand in both worlds: keep pointing at Docker while images come across,
//! then pin Apple's runtime — or install it from here without first having to
//! turn Docker off to be shown the offer.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, Context, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::AppState;
use crate::theme;
use model::{EngineChoice, EngineState, RuntimeKind};

pub struct Settings {
    state: AppState,
    busy: bool,
    notice: Option<String>,
    /// Engines this machine could be pointed at.
    choices: Vec<EngineChoice>,
    /// The pinned engine id, or `None` for automatic.
    preference: Option<String>,
    /// An Apple Containers install is downloading; macOS takes over after.
    installing: bool,
}

impl Settings {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.engine);
        watch(cx, &state.settings);
        let preference = state.host.settings().engine_preference;
        let view = Self {
            state,
            busy: false,
            notice: None,
            choices: Vec::new(),
            preference,
            installing: false,
        };
        view.load_choices(cx);
        view
    }

    fn load_choices(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        bridge::run(cx, async move { host.engine_choices().await }, move |choices, cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.choices = choices;
                    cx.notify();
                });
            }
        });
    }

    /// Pin an engine, or hand selection back to Hopper.
    ///
    /// The whole app follows: the client is repointed, the backend swaps, and
    /// the epoch bump makes every list refetch from whichever engine answered.
    fn choose(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.preference = id.clone();
        self.busy = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host.set_engine_preference(id).await },
            move |status, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.busy = false;
                        cx.notify();
                    });
                }
                state.engine.set(cx, status);
                state.settings.set(cx, state.host.settings());
                state.bump(cx);
            },
        );
    }

    /// Fetch Apple's signed installer and hand it to macOS.
    ///
    /// Reachable while Docker is connected and working — that is the point.
    /// Someone happy on Docker Desktop still needs a way to be offered the
    /// native runtime, and the first-run panel only shows when nothing is up.
    #[cfg(target_os = "macos")]
    fn install_apple(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let state = self.state.clone();
        self.installing = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host::appleinstall::download_and_open().await },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.installing = false;
                        this.notice = result.err();
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }

    /// One engine in the picker.
    ///
    /// Every listed engine is pickable, including one that is not installed —
    /// pinning the engine you are moving *to* is the whole point, and the
    /// status then says what is left to do about it.
    fn engine_button(
        &self,
        id: Option<&str>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.preference.as_deref() == id;
        let pin = id.map(str::to_string);
        Button::new(
            SharedString::from(format!("engine-choice-{}", id.unwrap_or("auto"))),
            label.to_string(),
        )
        .size(Size::Xs)
        .variant(if active { Variant::Light } else { Variant::Subtle })
        .color(if active { ColorName::Blue } else { ColorName::Gray })
        .disabled(self.busy)
        .on_click(cx.listener(move |this, _, _, cx| this.choose(pin.clone(), cx)))
    }

    fn start_engine(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.busy = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host.start_engine().await.map_err(|e| e.to_string()) },
            move |result, cx| {
                // A start that fails has to say so here. The button is in this
                // pane, so a `tracing::warn` reaches nobody who pressed it.
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.busy = false;
                        this.notice = result.err();
                        cx.notify();
                    });
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

        // `NotInstalled` is deliberately absent: on macOS that means Apple's
        // runtime is not on the machine, and there is nothing to start until
        // it is. The install button below is what that state deserves.
        let can_start = matches!(
            engine.state,
            EngineState::Stopped | EngineState::Unreachable
        ) && engine.managed;
        let can_stop = engine.connected && engine.managed;

        let mut engine_body = Stack::new()
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

        if let Some(notice) = &self.notice {
            let red = guise::theme::theme(cx).color(ColorName::Red, 6);
            engine_body = engine_body.child(Text::new(notice.clone()).size(Size::Xs).color(red));
        }

        // Which engine answers. Automatic is the default and stays first: it
        // is what puts a Mac on Apple's runtime without anyone choosing.
        let mut buttons = Group::new()
            .gap(Size::Xs)
            .child(self.engine_button(None, "Automatic", cx));
        for choice in &self.choices {
            buttons = buttons.child(self.engine_button(Some(&choice.id), &choice.label, cx));
        }

        let mut choice_body = Stack::new().gap(Size::Sm).child(buttons).child(
            Text::new(match self.preference.as_deref() {
                None => "Hopper picks the engine this machine is best on, and \
                         falls back to whatever is already running.",
                Some(_) => "Pinned. Hopper will not move off this engine on its own.",
            })
            .size(Size::Xs)
            .dimmed(),
        );

        // One row per engine: where it listens, or why it cannot be picked.
        // A machine with Docker Desktop and Podman both installed shows two
        // buttons that would otherwise say nothing but "Connected." — the
        // socket is what tells them apart. And an engine that is missing says
        // so, rather than leaving a dead button with no explanation.
        let mut rows = Stack::new().gap(Size::Xs);
        for choice in &self.choices {
            let detail = match (&choice.endpoint, &choice.reason) {
                (Some(endpoint), _) if choice.available => endpoint.clone(),
                // Apple's runtime is the one engine with nowhere to point.
                (None, _) if choice.available => {
                    "No Docker socket; Hopper drives this engine directly.".to_string()
                }
                (_, Some(reason)) => reason.clone(),
                _ => continue,
            };
            rows = rows.child(
                Group::new()
                    .gap(Size::Sm)
                    .align(Align::Center)
                    .child(
                        div()
                            .min_w(px(150.0))
                            .child(Text::new(choice.label.clone()).size(Size::Xs)),
                    )
                    .child(Text::new(detail).size(Size::Xs).dimmed()),
            );
        }
        choice_body = choice_body.child(rows);

        // The way off Docker Desktop, offered while Docker is still working.
        // Gated on the Mac being able to run it: an install cannot fix macOS 25.
        #[cfg(target_os = "macos")]
        if self.choices.iter().any(|c| c.managed && !c.available)
            && apple::system::too_old().is_none()
        {
            choice_body = choice_body.child(
                Group::new().gap(Size::Xs).child(
                    Button::new(
                        "engine-install-apple",
                        if self.installing {
                            "Downloading…"
                        } else {
                            "Install Apple Containers"
                        },
                    )
                    .size(Size::Xs)
                    .variant(Variant::Filled)
                    .color(ColorName::Blue)
                    .disabled(self.installing)
                    .on_click(cx.listener(|this, _, _, cx| this.install_apple(cx))),
                ),
            );
        }

        // Apple's runtime answers no socket, so there is nothing for
        // `DOCKER_HOST` to name — printing the engine's label there would be a
        // line that looks like a command and works like nothing.
        let cli_body = if self.state.host.runtime_kind() == RuntimeKind::Apple {
            Stack::new()
                .gap(Size::Xs)
                .child(Text::new("container ls").size(Size::Xs))
                .child(
                    Text::new(
                        "Apple Containers publishes no Docker socket, so `docker` cannot be \
                         pointed at it. Apple's own `container` command drives the same \
                         runtime Hopper is showing.",
                    )
                    .size(Size::Xs)
                    .dimmed(),
                )
        } else {
            Stack::new()
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
                )
        };

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
                div().id("settings-scroll").flex_1().overflow_y_scroll().p_4().child(
                    Stack::new()
                        .gap(Size::Md)
                        .child(self.section("Engine", engine_body, cx))
                        .child(self.section("Which engine", choice_body, cx))
                        .child(self.section("Docker CLI", cli_body, cx))
                        .child(self.section("Appearance", theme_body, cx)),
                ),
            )
    }
}
