//! The UI tree. `Root` owns routing, installs the app-wide context, runs the
//! engine-status poll, and drives the Docker event stream that keeps every
//! list live.

use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{div, Context, Entity, Window};
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
    settings: Entity<views::Settings>,
    detail: Option<Entity<views::Detail>>,
    volumes: Entity<views::Volumes>,
    networks: Entity<views::Networks>,
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
        let settings = cx.new(views::Settings::new);
        let volumes = cx.new(views::Volumes::new);
        let networks = cx.new(views::Networks::new);

        let root = Root {
            state,
            containers,
            images,
            dashboard,
            stacks,
            settings,
            volumes,
            networks,
            detail: None,
        };
        root.bring_up_engine(cx);
        root.poll_engine(cx);
        root.watch_events(cx);
        root
    }

    /// Select an engine on launch, and start Hopper's own if that is what the
    /// active provider is and the user has autostart on.
    ///
    /// Without this the app only ever probes the default socket, so a user
    /// with no Docker installed would never reach the managed VM engine — the
    /// whole point of being a Docker Desktop replacement.
    fn bring_up_engine(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let engine = self.state.engine.clone();
        cx.spawn(async move |_, cx| {
            // Pick the provider (managed VM where available, else an existing
            // engine) and point the client at it.
            let status = {
                let host = Arc::clone(&host);
                let (tx, rx) = futures::channel::oneshot::channel();
                bridge::runtime().spawn(async move { let _ = tx.send(host.select_engine().await); });
                rx.await.ok()
            };
            let Some(status) = status else { return };
            let _ = cx.update(|cx| engine.set(cx, status.clone()));

            // Start the managed engine if it is selected, idle, and autostart
            // is on. An existing engine someone else runs is left alone.
            let autostart = host.settings().autostart_engine;
            if autostart && status.managed && !status.connected {
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
    /// Hopper is reflected without the user reloading anything.
    fn poll_engine(&self, cx: &mut Context<Self>) {
        let state = self.state.clone();
        cx.spawn(async move |_, cx| loop {
            let host = Arc::clone(&state.host);
            let (tx, rx) = futures::channel::oneshot::channel();
            bridge::runtime().spawn(async move {
                let _ = tx.send(host.engine_status().await);
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.global::<Theme>();
        let body = t.body().hsla();
        let text = t.text().hsla();
        let font = t.font_family.clone();

        let route = self.state.route.get(cx);
        let content: gpui::Div = match route {
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
            Route::Volumes => div().size_full().child(self.volumes.clone()),
            Route::Networks => div().size_full().child(self.networks.clone()),
            Route::Dashboard => div().size_full().child(self.dashboard.clone()),
            Route::Stacks => div().size_full().child(self.stacks.clone()),
            Route::Settings => div().size_full().child(self.settings.clone()),
        };

        div()
            .relative()
            .size_full()
            .flex()
            .bg(body)
            .text_color(text)
            .font_family(font)
            .child(sidebar::render(&self.state, cx))
            .child(content)
    }
}
