//! The UI tree. `Root` owns routing, installs the app-wide context, runs the
//! engine-status poll, and drives the Docker event stream that keeps every
//! list live.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{div, Context, Entity, FocusHandle, Window};
use guise::prelude::*;

use crate::bridge;
use crate::sidebar;
use crate::state::{AppState, Route};
use crate::views;
use host::Host;

pub struct Root {
    state: AppState,
    containers: Entity<views::Containers>,
    images: Entity<views::Images>,
    dashboard: Entity<views::Dashboard>,
    stacks: Entity<views::Stacks>,
    import: Entity<views::Import>,
    settings: Entity<views::Settings>,
    detail: Option<Entity<views::Detail>>,
    volumes: Entity<views::Volumes>,
    networks: Entity<views::Networks>,
    registry: Entity<views::Registry>,
    engine_setup: Entity<views::EngineSetup>,
    run_dialog: Entity<views::RunDialog>,
    /// The window root's focus. gpui dispatches actions along the focus path,
    /// so with nothing focused the menu bar greys out and its shortcuts are
    /// swallowed.
    focus: FocusHandle,
}

impl Root {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let host = Host::from_env();
        let state = AppState::new(Arc::clone(&host), cx);
        provide(cx, state.clone());
        watch(cx, &state.route);
        watch(cx, &state.engine);

        let containers = cx.new(views::Containers::new);
        let images = cx.new(views::Images::new);
        let dashboard = cx.new(views::Dashboard::new);
        let stacks = cx.new(views::Stacks::new);
        let import = cx.new(views::Import::new);
        let settings = cx.new(views::Settings::new);
        let volumes = cx.new(views::Volumes::new);
        let networks = cx.new(views::Networks::new);
        let registry = cx.new(views::Registry::new);
        let engine_setup = cx.new(views::EngineSetup::new);
        let run_dialog = cx.new(views::RunDialog::new);
        let focus = cx.focus_handle();

        let root = Root {
            state,
            containers,
            images,
            dashboard,
            stacks,
            import,
            settings,
            volumes,
            networks,
            registry,
            engine_setup,
            run_dialog,
            detail: None,
            focus,
        };
        root.bring_up_engine(cx);
        root.poll_engine(cx);
        root.watch_events(cx);
        root
    }

    /// Select an engine on launch, and start it if it is one Hopper manages
    /// and the user has autostart on.
    ///
    /// Without this the app only ever probes the default socket, so a Mac with
    /// no Docker would never reach Apple's runtime — the whole point of being
    /// a Docker Desktop replacement.
    fn bring_up_engine(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let engine = self.state.engine.clone();
        cx.spawn(async move |_, cx| {
            // Pick the provider (Apple's runtime where available, else an
            // engine someone else runs) and point the client at it.
            let status = {
                let host = Arc::clone(&host);
                let (tx, rx) = futures::channel::oneshot::channel();
                bridge::runtime().spawn(async move { let _ = tx.send(host.select_engine().await); });
                rx.await.ok()
            };
            let Some(status) = status else { return };
            let _ = cx.update(|cx| engine.set(cx, status.clone()));

            // Start the managed engine if it is selected, installed but idle,
            // and autostart is on. An engine someone else runs is left alone,
            // and one that is not installed has nothing to start — the setup
            // panel offers the install instead.
            let autostart = host.settings().autostart_engine;
            if autostart && status.managed && status.state == model::EngineState::Stopped {
                let host = Arc::clone(&host);
                let (tx, rx) = futures::channel::oneshot::channel();
                bridge::runtime().spawn(async move {
                    let _ = tx.send(host.start_engine().await.map_err(|e| e.to_string()));
                });
                if let Ok(Err(e)) = rx.await {
                    tracing::warn!("engine autostart failed: {e}");
                }
            }
        })
        .detach();
    }

    /// Probe the engine on a timer so a daemon starting or stopping outside
    /// Hopper is reflected without the user reloading anything — and so an
    /// engine that appears after launch is selected without a restart.
    fn poll_engine(&self, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |_, cx| loop {
            let host = Arc::clone(&state.host);
            let (tx, rx) = futures::channel::oneshot::channel();
            bridge::runtime().spawn(async move {
                let _ = tx.send(host.poll_engine().await);
            });
            if let Ok(status) = rx.await {
                if cx.update(|cx| state.engine.set(cx, status)).is_err() {
                    return;
                }
            }
            cx.background_executor()
                .timer(Duration::from_secs(3))
                .await;
        })
        .detach();
    }

    /// Follow the daemon's event firehose and bump the refresh epoch.
    ///
    /// Events are coalesced: a `compose up` of twenty services emits hundreds
    /// of events in a second, and refetching per event would hammer the daemon
    /// and thrash the list.
    fn watch_events(&self, cx: &mut Context<Self>) {
        let state = self.state.clone();
        let host = Arc::clone(&self.state.host);
        bridge::stream(
            cx,
            move |tx| async move {
                let _ = host
                    .stream_events(|event| {
                        // A closed receiver means the app is gone.
                        tx.unbounded_send(event).is_ok()
                    })
                    .await;
            },
            move |_event, cx| {
                state.bump(cx);
                // A container that publishes a port has to become reachable
                // without the user doing anything, so the forwarder is driven
                // off the same events the lists refresh from.
                let host = Arc::clone(&state.host);
                bridge::run(cx, async move { host.resync_forwards().await }, |failures, _| {
                    for reason in failures {
                        tracing::warn!("{reason}");
                    }
                });
            },
            |_| {},
        );
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.global::<Theme>();
        let body = t.body().hsla();
        let text = t.text().hsla();
        let font = t.font_family.clone();

        let route = self.state.route.get(cx);
        let engine = self.state.engine.get(cx);

        // With no engine answering, stand in for the resource lists with the
        // first-run surface — what's happening and what to do — rather than a
        // list that failed to load. Settings stays itself (the engine is
        // configured there), and Registry stays reachable so you can browse and
        // queue images to pull before the engine is even up.
        let show_setup = !engine.connected
            && route != Route::Settings
            && route != Route::Registry
            && route != Route::Import;

        let content: gpui::Div = if show_setup {
            div().size_full().child(self.engine_setup.clone())
        } else {
            match route {
            Route::Containers => {
                // The detail pane sits beside the list, so a container's logs
                // are visible without losing the list you came from.
                let selected = self.state.selected.get(cx);
                let mut split = div().size_full().flex().child(
                    div().flex_1().overflow_hidden().child(self.containers.clone()),
                );
                if let Some(container) = selected {
                    let pane = match self.detail.clone() {
                        Some(pane) => {
                            pane.update(cx, |pane, cx| pane.show(container, cx));
                            pane
                        }
                        None => {
                            let pane = cx.new(|cx| views::Detail::new(container, cx));
                            self.detail = Some(pane.clone());
                            pane
                        }
                    };
                    split = split.child(div().w(gpui::px(520.0)).h_full().child(pane));
                } else {
                    // Nothing selected: drop the pane so its streams stop.
                    self.detail = None;
                }
                split
            }
                Route::Images => div().size_full().child(self.images.clone()),
                Route::Registry => div().size_full().child(self.registry.clone()),
                Route::Volumes => div().size_full().child(self.volumes.clone()),
                Route::Networks => div().size_full().child(self.networks.clone()),
                Route::Dashboard => div().size_full().child(self.dashboard.clone()),
                Route::Stacks => div().size_full().child(self.stacks.clone()),
                Route::Import => div().size_full().child(self.import.clone()),
                Route::Settings => div().size_full().child(self.settings.clone()),
            }
        };

        // Take focus when nothing has it, so the menu bar stays live.
        if window.focused(cx).is_none() {
            window.focus(&self.focus);
        }

        // The menu declares Settings… and Refresh and binds ⌘, and ⌘R to them,
        // but nothing ever handled either — both items rendered greyed and
        // both shortcuts did nothing. gpui dispatches along the focus path,
        // which is what `track_focus` above puts this element on.
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &crate::OpenSettings, _, cx| {
                this.state.route.set(cx, Route::Settings)
            }))
            .on_action(cx.listener(|this, _: &crate::Refresh, _, cx| this.state.bump(cx)))
            .relative()
            .size_full()
            .flex()
            .bg(body)
            .text_color(text)
            .font_family(font)
            .child(sidebar::render(&self.state, cx))
            .child(content)
            // Overlays, above everything: the Run dialog (renders nothing when
            // closed) and the app-wide toast stack (top-right).
            .child(self.run_dialog.clone())
            .child(self.state.toasts.clone())
    }
}
