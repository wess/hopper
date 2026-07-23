//! The networks list.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::{AppState, Load};
use crate::theme;
use model::Network;

pub struct Networks {
    state: AppState,
    last_epoch: u64,
    busy: Option<String>,
}

impl Networks {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.networks);
        watch(cx, &state.epoch);
        watch(cx, &state.search);

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
        let signal = self.state.networks.clone();
        bridge::run(cx, async move { host.networks().await }, move |result, cx| {
            signal.set(
                cx,
                match result {
                    Ok(list) => Load::Ready(list),
                    Err(e) => Load::Failed(e.message),
                },
            );
        });
    }

    fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = Some(id.clone());
        cx.notify();
        bridge::run(
            cx,
            async move { host.network_remove(&id).await },
            move |result, cx| {
                if let Err(e) = result {
                    tracing::warn!("network remove failed: {}", e.message);
                }
                state.bump(cx);
            },
        );
    }

    fn row(&self, n: &Network, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let busy = self.busy.as_deref() == Some(n.id.as_str());
        let builtin = n.is_builtin();
        let id = n.id.clone();

        let mut badges = Group::new().gap(Size::Xs).child(
            Badge::new(n.driver.clone())
                .variant(Variant::Light)
                .color(ColorName::Gray)
                .size(Size::Xs),
        );
        if builtin {
            badges = badges.child(
                Badge::new("built-in")
                    .variant(Variant::Outline)
                    .color(ColorName::Gray)
                    .size(Size::Xs),
            );
        }
        if n.internal {
            badges = badges.child(
                Badge::new("internal")
                    .variant(Variant::Light)
                    .color(ColorName::Orange)
                    .size(Size::Xs),
            );
        }

        let subnet = n
            .ipam
            .iter()
            .filter_map(|i| i.subnet.clone())
            .collect::<Vec<_>>()
            .join(", ");

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .p_3()
            .border_b_1()
            .border_color(palette.border_subtle)
            .child(
                Stack::new()
                    .gap(Size::Xs)
                    .child(
                        Group::new()
                            .gap(Size::Xs)
                            .align(Align::Center)
                            .child(Text::new(n.name.clone()).size(Size::Sm).medium())
                            .child(badges),
                    )
                    .child(
                        Text::new(if subnet.is_empty() {
                            format!("{} containers", n.containers)
                        } else {
                            format!("{}  ·  {} containers", subnet, n.containers)
                        })
                        .size(Size::Xs)
                        .dimmed(),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("rm-{}", n.id)), "Remove")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .color(ColorName::Red)
                    // Docker's own networks can never be removed.
                    .disabled(busy || builtin)
                    .on_click(cx.listener(move |this, _, _, cx| this.remove(id.clone(), cx))),
            )
    }
}

impl Render for Networks {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.busy = None;
            self.reload(cx);
        }

        let palette = theme::palette(cx);
        let load = self.state.networks.get(cx);
        let query = self.state.search.get(cx).trim().to_lowercase();

        let body = match load {
            Load::Loading => crate::views::message("Loading networks…"),
            Load::Failed(e) => crate::views::failure("Could not list networks", &e),
            Load::Ready(list) => {
                let filtered: Vec<&Network> = list
                    .iter()
                    .filter(|n| query.is_empty() || n.name.to_lowercase().contains(&query))
                    .collect();
                if filtered.is_empty() {
                    crate::views::message("No networks match your search.")
                } else {
                    let mut rows = div().flex().flex_col();
                    for n in filtered {
                        rows = rows.child(self.row(n, cx));
                    }
                    rows.into_any_element()
                }
            }
        };

        let count = self
            .state
            .networks
            .get(cx)
            .ready()
            .map(|l| l.len())
            .unwrap_or(0);

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
                    .child(
                        Group::new()
                            .gap(Size::Xs)
                            .align(Align::Center)
                            .child(Text::new("Networks").size(Size::Xl).bold())
                            .child(
                                Badge::new(count.to_string())
                                    .variant(Variant::Light)
                                    .color(ColorName::Gray)
                                    .size(Size::Xs),
                            ),
                    ),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}
