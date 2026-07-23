//! The containers list.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::{filter_containers, AppState, Load};
use crate::theme;
use model::{Container, Health};

pub struct Containers {
    state: AppState,
    last_epoch: u64,
    /// Set while a lifecycle action is in flight, so the row can disable its
    /// buttons instead of letting the user queue five stops.
    busy: Option<String>,
}

impl Containers {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.containers);
        watch(cx, &state.epoch);
        watch(cx, &state.search);
        watch(cx, &state.show_all);
        watch(cx, &state.selected);

        let view = Self {
            state,
            last_epoch: 0,
            busy: None,
        };
        view.reload(cx);
        view
    }

    fn reload(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let all = self.state.show_all.get(cx);
        let signal = self.state.containers.clone();
        let state = self.state.clone();
        bridge::run(
            cx,
            async move { host.containers(all).await },
            move |result, cx| {
                let next = match result {
                    Ok(list) => {
                        // Dev-only: auto-open the first running container's
                        // detail pane, so the detail tabs can be screenshotted.
                        if std::env::var("HOPPER_SELECT").is_ok() {
                            if let Some(c) = list.iter().find(|c| c.state.is_up()).or_else(|| list.first()) {
                                if state.selected.get(cx).is_none() {
                                    state.selected.set(cx, Some(c.clone()));
                                }
                            }
                        }
                        Load::Ready(list)
                    }
                    Err(e) => Load::Failed(e.message),
                };
                signal.set(cx, next);
            },
        );
    }

    fn act(&mut self, id: String, action: Action, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = Some(id.clone());
        cx.notify();

        let for_call = id.clone();
        bridge::run(
            cx,
            async move {
                match action {
                    Action::Start => host.container_start(&for_call).await,
                    Action::Stop => host.container_stop(&for_call).await,
                    Action::Restart => host.container_restart(&for_call).await,
                }
            },
            move |result, cx| {
                if let Err(e) = result {
                    tracing::warn!("container action failed: {}", e.message);
                }
                // Refetch either way: a failure still may have changed state.
                state.bump(cx);
            },
        );
    }

    fn row(&self, c: &Container, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let up = c.state.is_up();
        let busy = self.busy.as_deref() == Some(c.id.as_str());

        let id_primary = c.id.clone();
        let id_restart = c.id.clone();

        let mut badges = Group::new().gap(Size::Xs).child(
            Badge::new(c.state.as_str())
                .variant(Variant::Light)
                .color(theme::state_color(c.state))
                .size(Size::Xs),
        );
        // A running-but-unhealthy container must not read as plain green.
        if let Some(accent) = theme::health_color(c.health) {
            if c.health != Health::None {
                badges = badges.child(
                    Badge::new(c.health.as_str())
                        .variant(Variant::Light)
                        .color(accent)
                        .size(Size::Xs),
                );
            }
        }
        if let Some(project) = c.compose_project.as_ref() {
            badges = badges.child(
                Badge::new(project.clone())
                    .variant(Variant::Outline)
                    .color(ColorName::Gray)
                    .size(Size::Xs),
            );
        }

        let ports = c
            .ports
            .iter()
            .filter_map(|p| p.public_port.map(|pub_port| format!("{pub_port}→{}", p.private_port)))
            .collect::<Vec<_>>()
            .join(", ");

        let primary = Button::new(
            SharedString::from(format!("toggle-{}", c.id)),
            if up { "Stop" } else { "Start" },
        )
        .size(Size::Xs)
        .variant(Variant::Light)
        .color(if up { ColorName::Red } else { ColorName::Green })
        .disabled(busy)
        .on_click(cx.listener(move |this, _, _, cx| {
            let action = if up { Action::Stop } else { Action::Start };
            this.act(id_primary.clone(), action, cx);
        }));

        let restart = Button::new(SharedString::from(format!("restart-{}", c.id)), "Restart")
            .size(Size::Xs)
            .variant(Variant::Subtle)
            .color(ColorName::Gray)
            .disabled(busy || !up)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.act(id_restart.clone(), Action::Restart, cx);
            }));

        let selected = self.state.selected.get(cx).map(|c| c.id) == Some(c.id.clone());
        let for_select = c.clone();
        let selected_signal = self.state.selected.clone();

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .p_3()
            .border_b_1()
            .border_color(palette.border_subtle)
            .when(selected, |d| d.bg(palette.bg_muted))
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                // Clicking the open row closes the pane, so the list can be
                // given the full width again.
                selected_signal.update(cx, |current| {
                    *current = if current.as_ref().map(|c| &c.id) == Some(&for_select.id) {
                        None
                    } else {
                        Some(for_select.clone())
                    };
                });
            })
            .child(
                Stack::new()
                    .gap(Size::Xs)
                    .child(
                        Group::new()
                            .gap(Size::Xs)
                            .align(Align::Center)
                            .child(Text::new(c.name.clone()).size(Size::Sm).medium())
                            .child(badges),
                    )
                    .child(
                        Text::new(if ports.is_empty() {
                            c.image.clone()
                        } else {
                            format!("{}  ·  {}", c.image, ports)
                        })
                        .size(Size::Xs)
                        .dimmed(),
                    ),
            )
            .child(Group::new().gap(Size::Xs).child(primary).child(restart))
    }

    fn body(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let load = self.state.containers.get(cx);
        let query = self.state.search.get(cx);

        match load {
            Load::Loading => div()
                .p_6()
                .child(Text::new("Loading containers…").size(Size::Sm).dimmed())
                .into_any_element(),

            Load::Failed(err) => div()
                .p_6()
                .child(
                    Stack::new()
                        .gap(Size::Xs)
                        .child(
                            Text::new("Could not reach the Docker engine")
                                .size(Size::Sm)
                                .medium(),
                        )
                        // The daemon's own words, not a generic failure line.
                        .child(Text::new(err).size(Size::Xs).dimmed()),
                )
                .into_any_element(),

            Load::Ready(list) => {
                let filtered = filter_containers(&list, &query);
                if filtered.is_empty() {
                    let message = if list.is_empty() {
                        "No containers yet."
                    } else {
                        "No containers match your search."
                    };
                    return div()
                        .p_6()
                        .child(Text::new(message).size(Size::Sm).dimmed())
                        .into_any_element();
                }
                let mut rows = div().flex().flex_col();
                for c in filtered {
                    rows = rows.child(self.row(c, cx));
                }
                rows.into_any_element()
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    Start,
    Stop,
    Restart,
}

impl Render for Containers {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The event stream bumps the epoch; refetch when it moves.
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.busy = None;
            self.reload(cx);
        }

        let palette = theme::palette(cx);
        let count = self
            .state
            .containers
            .get(cx)
            .ready()
            .map(|l| l.len())
            .unwrap_or(0);

        let show_all = self.state.show_all.get(cx);
        let all_signal = self.state.show_all.clone();

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .p_4()
            .border_b_1()
            .border_color(palette.border)
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .align(Align::Center)
                    .child(Text::new("Containers").size(Size::Xl).bold())
                    .child(
                        Badge::new(count.to_string())
                            .variant(Variant::Light)
                            .color(ColorName::Gray)
                            .size(Size::Xs),
                    ),
            )
            .child(
                Button::new(
                    "toggle-all",
                    if show_all { "Showing all" } else { "Running only" },
                )
                .size(Size::Xs)
                .variant(Variant::Subtle)
                .on_click(move |_, _, cx| all_signal.update(cx, |v| *v = !*v)),
            );

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(header)
            .child(div().flex_1().overflow_hidden().child(self.body(cx)))
    }
}
