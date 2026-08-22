//! The first-run engine surface.
//!
//! When no Docker engine is answering, this stands in for the resource lists
//! and says what is actually happening — Hopper is setting up its own engine,
//! it is ready to start, or it can't run here and why — instead of a list that
//! failed to load with a mute status dot in the corner.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, Context, Window};
use guise::prelude::*;
use model::{EngineState, EngineStatus};

use crate::bridge;
use crate::state::AppState;
use crate::theme;

/// What to show for an engine that isn't connected: a heading, the engine's own
/// words, an optional secondary hint, whether we are mid-setup, and whether the
/// user can start it from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    pub icon: IconName,
    pub tone: ColorName,
    pub title: String,
    pub body: String,
    pub hint: Option<String>,
    /// Setup is in flight — show a spinner, not a startable state.
    pub busy: bool,
    /// What the user can do about it from here.
    pub offer: Offer,
}

/// The action this surface puts in front of the user.
///
/// A missing engine and a stopped one both look like "not working", but only
/// one of them is fixed by a Start button — offering the wrong one is worse
/// than offering nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// Nothing to do here; the text explains why.
    Nothing,
    /// A managed engine that is installed and can be started.
    Start,
    /// Apple's runtime is not on the machine yet.
    Install,
}

impl Setup {
    pub fn can_start(&self) -> bool {
        self.offer == Offer::Start
    }
    pub fn can_install(&self) -> bool {
        self.offer == Offer::Install
    }
}

/// Map an engine status to the first-run surface. Pure so the wording and the
/// start/spinner decisions are unit-tested rather than eyeballed.
pub fn describe(e: &EngineStatus) -> Setup {
    use EngineState::*;
    let body = e.message.clone();
    let hint = e.detail.clone();
    let apple = e.provider == "apple";
    match e.state {
        Starting => Setup {
            icon: IconName::HardDriveDownload,
            tone: ColorName::Blue,
            title: if apple { "Starting Apple Containers".into() } else { "Connecting to the engine".into() },
            body,
            hint,
            busy: true,
            offer: Offer::Nothing,
        },
        Stopped if e.managed => Setup {
            icon: IconName::Power,
            tone: ColorName::Green,
            title: if apple { "Start Apple Containers".into() } else { "Start Hopper's engine".into() },
            body,
            hint,
            busy: false,
            offer: Offer::Start,
        },
        Unsupported => Setup {
            icon: IconName::TriangleAlert,
            tone: ColorName::Orange,
            title: if apple {
                "Apple Containers can't run on this Mac".into()
            } else {
                "Hopper can't run its own engine here".into()
            },
            body,
            hint,
            busy: false,
            offer: Offer::Nothing,
        },
        NeedsPermission => Setup {
            icon: IconName::TriangleAlert,
            tone: ColorName::Orange,
            title: "Hopper needs permission to reach Docker".into(),
            body,
            hint,
            busy: false,
            offer: Offer::Nothing,
        },
        // Apple's runtime is the macOS engine, so "not installed" is an
        // offer to install it rather than a dead end.
        NotInstalled if apple => Setup {
            icon: IconName::HardDriveDownload,
            tone: ColorName::Blue,
            title: "Run containers natively on this Mac".into(),
            body,
            hint,
            busy: false,
            offer: Offer::Install,
        },
        NotInstalled => Setup {
            icon: IconName::CircleAlert,
            tone: ColorName::Gray,
            title: "No Docker engine found".into(),
            body,
            hint,
            busy: false,
            offer: if e.managed { Offer::Start } else { Offer::Nothing },
        },
        Unreachable => Setup {
            icon: IconName::CircleAlert,
            tone: ColorName::Orange,
            title: "Docker engine isn't responding".into(),
            body,
            hint,
            busy: false,
            offer: if e.managed { Offer::Start } else { Offer::Nothing },
        },
        Stopped => Setup {
            icon: IconName::Power,
            tone: ColorName::Gray,
            title: "Docker engine isn't running".into(),
            body,
            hint,
            busy: false,
            offer: Offer::Nothing,
        },
        // Only rendered when disconnected, so this arm is a formality.
        Connected => Setup {
            icon: IconName::Container,
            tone: ColorName::Green,
            title: "The engine is running".into(),
            body,
            hint,
            busy: false,
            offer: Offer::Nothing,
        },
    }
}

/// Hand a URL to the system browser.
#[cfg(target_os = "macos")]
fn open_url(url: &str) {
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
}

pub struct EngineSetup {
    state: AppState,
    /// A start is in flight — hold the button until the poll reflects it.
    starting: bool,
    /// An install is downloading. macOS takes over once it opens.
    installing: bool,
    /// What went wrong with the last install attempt.
    install_error: Option<String>,
}

impl EngineSetup {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.engine);
        Self {
            state,
            starting: false,
            installing: false,
            install_error: None,
        }
    }

    /// Fetch Apple's signed installer and hand it to macOS.
    ///
    /// Hopper never elevates: the package asks for administrator rights
    /// itself, and the user approves it in the system installer.
    #[cfg(target_os = "macos")]
    fn install(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let state = self.state.clone();
        self.installing = true;
        self.install_error = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host::appleinstall::download_and_open().await },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.installing = false;
                        this.install_error = result.err();
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.starting = true;
        cx.notify();
        bridge::run(
            cx,
            async move { host.start_engine().await.map_err(|e| e.to_string()) },
            move |result, cx| {
                if let Err(e) = result {
                    tracing::warn!("engine start failed: {e}");
                }
                // Re-enable the button; the poll will have moved the engine to
                // "starting" and hidden it anyway, but a failed start must not
                // leave it stuck.
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.starting = false;
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }
}

impl Render for EngineSetup {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let setup = describe(&self.state.engine.get(cx));
        // Read the offer before the card consumes the setup's strings.
        let (can_install, can_start) = (setup.can_install(), setup.can_start());

        let leading = if setup.busy {
            Loader::new()
                .size(Size::Md)
                .color(ColorName::Blue)
                .into_any_element()
        } else {
            Icon::new(setup.icon)
                .size(Size::Lg)
                .color(setup.tone)
                .into_any_element()
        };

        let mut card = Stack::new()
            .gap(Size::Sm)
            .child(
                Group::new()
                    .gap(Size::Sm)
                    .align(Align::Center)
                    .child(leading)
                    .child(Text::new(setup.title).size(Size::Lg).medium()),
            )
            .child(Text::new(setup.body).size(Size::Sm));

        if let Some(hint) = setup.hint {
            card = card.child(Text::new(hint).size(Size::Xs).dimmed());
        }
        if can_install {
            #[cfg(target_os = "macos")]
            {
                card = card.child(
                    Group::new()
                        .gap(Size::Xs)
                        .child(
                            Button::new(
                                "engine-setup-install",
                                if self.installing {
                                    "Downloading…"
                                } else {
                                    "Install Apple Containers"
                                },
                            )
                            .size(Size::Sm)
                            .variant(Variant::Filled)
                            .color(ColorName::Blue)
                            .disabled(self.installing)
                            .on_click(cx.listener(|this, _, _, cx| this.install(cx))),
                        )
                        .child(
                            Anchor::new("engine-setup-learn", "What is this?")
                                .size(Size::Xs)
                                .on_click(|_, _, _| open_url(apple::HOMEPAGE)),
                        ),
                );
            }
            if let Some(error) = &self.install_error {
                let red = guise::theme::theme(cx).color(ColorName::Red, 6);
                card = card.child(Text::new(error.clone()).size(Size::Xs).color(red));
            }
        }

        if can_start {
            card = card.child(
                Button::new(
                    "engine-setup-start",
                    if self.starting {
                        "Starting…"
                    } else {
                        "Start Hopper's engine"
                    },
                )
                .size(Size::Sm)
                .variant(Variant::Light)
                .color(ColorName::Green)
                .disabled(self.starting)
                .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
            );
        }

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .w(px(460.0))
                    .p_6()
                    .rounded_lg()
                    .bg(palette.bg_subtle)
                    .border_1()
                    .border_color(palette.border_subtle)
                    .child(card),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: EngineState, provider: &str, msg: &str) -> EngineStatus {
        EngineStatus::new(state, provider, msg)
    }

    #[test]
    fn a_starting_engine_reads_as_setup_in_flight() {
        let s = describe(
            &status(EngineState::Starting, "apple", "Starting Apple's services…").managed(true),
        );
        assert!(s.busy);
        assert!(!s.can_start());
        assert_eq!(s.title, "Starting Apple Containers");
    }

    #[test]
    fn a_stopped_managed_engine_offers_a_start() {
        let s = describe(&status(EngineState::Stopped, "apple", "not running").managed(true));
        assert!(s.can_start());
        assert!(!s.busy);
        assert_eq!(s.title, "Start Apple Containers");
    }

    #[test]
    fn an_unmanaged_stopped_engine_is_not_ours_to_start() {
        let s = describe(&status(EngineState::Stopped, "existing", "Docker Desktop is stopped."));
        assert!(!s.can_start());
    }

    #[test]
    fn an_unsupported_engine_explains_and_offers_no_start() {
        let s = describe(
            &status(EngineState::Unsupported, "apple", "needs macOS 26 or later").managed(true),
        );
        assert!(!s.can_start());
        assert!(!s.can_install(), "an install cannot fix an unsupported Mac");
        assert_eq!(s.tone, ColorName::Orange);
    }

    #[test]
    fn a_mac_without_apples_runtime_is_offered_the_install() {
        let s = describe(
            &status(EngineState::NotInstalled, "apple", "Apple Containers is not installed yet.")
                .managed(true),
        );
        assert!(s.can_install());
        assert!(!s.can_start(), "there is nothing installed to start yet");
        assert_eq!(s.tone, ColorName::Blue);
    }

    #[test]
    fn a_missing_docker_is_not_mistaken_for_a_missing_apple_runtime() {
        // Only the apple provider offers an install; everything else says so.
        let s = describe(&status(EngineState::NotInstalled, "existing", "No Docker engine."));
        assert!(!s.can_install());
        assert_eq!(s.title, "No Docker engine found");
    }

    #[test]
    fn a_dev_build_surfaces_the_managed_engine_hint() {
        // What the enriched status looks like in a `cargo run` build with no
        // Docker: existing is down, and the managed engine's reason rides along
        // in `detail` so the user learns to run the signed app.
        let e = EngineStatus::new(EngineState::NotInstalled, "existing", "No Docker engine is running.")
            .detail(
                "Hopper cannot run its own engine on this Mac. This build lacks the \
                 virtualization entitlement — run the signed Hopper.app.",
            );
        let s = describe(&e);
        assert_eq!(s.title, "No Docker engine found");
        assert!(!s.can_start(), "a dev build cannot start a VM");
        assert!(s.hint.unwrap().contains("signed Hopper.app"));
    }
}
