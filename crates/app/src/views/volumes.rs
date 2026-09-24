//! The volumes list.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::format;
use crate::state::{AppState, Load};
use crate::theme;
use model::Volume;

pub struct Volumes {
    state: AppState,
    last_epoch: u64,
    busy: Option<String>,
    confirm_remove: Option<String>,
}

impl Volumes {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.volumes);
        watch(cx, &state.epoch);
        watch(cx, &state.search);

        let view = Self {
            state,
            last_epoch: 0,
            busy: None,
            confirm_remove: None,
        };
        view.reload(cx);
        view
    }

    fn reload(&self, cx: &mut Context<Self>) {
        if !self.state.host.selection_ready() {
            return;
        }
        let host = Arc::clone(&self.state.host);
        let request_generation = host.selection_generation();
        let check_host = Arc::clone(&host);
        let signal = self.state.volumes.clone();
        bridge::run(
            cx,
            async move { host.volumes().await },
            move |result, cx| {
                if check_host.selection_generation() != request_generation {
                    return;
                }
                signal.set(
                    cx,
                    match result {
                        Ok(list) => Load::Ready(list),
                        Err(e) => Load::Failed(e.message),
                    },
                );
            },
        );
    }

    fn remove(&mut self, name: String, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = Some(name.clone());
        cx.notify();
        bridge::run(
            cx,
            async move { host.volume_remove(&name, false).await },
            move |result, cx| {
                if let Err(e) = result {
                    tracing::warn!("volume remove failed: {}", e.message);
                    state.toast_titled(cx, "Could not remove volume", e.message, ColorName::Red);
                }
                state.bump(cx);
            },
        );
    }

    fn ask_remove(&mut self, name: String, cx: &mut Context<Self>) {
        self.confirm_remove = Some(name);
        cx.notify();
    }

    fn cancel_remove(&mut self, cx: &mut Context<Self>) {
        self.confirm_remove = None;
        cx.notify();
    }

    fn row(&self, v: &Volume, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let busy = self.busy.as_deref() == Some(v.name.as_str());
        let name = v.name.clone();

        let mut badges = Group::new().gap(Size::Xs).child(
            Badge::new(v.driver.clone())
                .variant(Variant::Light)
                .color(ColorName::Gray)
                .size(Size::Xs),
        );
        if v.in_use {
            badges = badges.child(
                Badge::new("in use")
                    .variant(Variant::Light)
                    .color(ColorName::Blue)
                    .size(Size::Xs),
            );
        }

        div()
            .flex()
            .flex_none()
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
                            .child(Text::new(v.name.clone()).size(Size::Sm).medium())
                            .child(badges),
                    )
                    .child(
                        Text::new(format!("{}  ·  {}", format::bytes(v.size), v.mountpoint))
                            .size(Size::Xs)
                            .dimmed(),
                    ),
            )
            .child(
                Button::new(SharedString::from(format!("rm-{}", v.name)), "Remove")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .color(ColorName::Red)
                    // A volume a container still mounts cannot be removed, and
                    // offering it would only produce a daemon error.
                    .disabled(busy || v.in_use)
                    .on_click(cx.listener(move |this, _, _, cx| this.ask_remove(name.clone(), cx))),
            )
    }
}

impl Render for Volumes {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.busy = None;
            self.reload(cx);
        }

        let palette = theme::palette(cx);
        let load = self.state.volumes.get(cx);
        let query = self.state.search.get(cx).trim().to_lowercase();

        let body = match load {
            Load::Loading => crate::views::message("Loading volumes…"),
            Load::Failed(e) => crate::views::failure("Could not list volumes", &e),
            Load::Ready(list) => {
                let filtered: Vec<&Volume> = list
                    .iter()
                    .filter(|v| query.is_empty() || v.name.to_lowercase().contains(&query))
                    .collect();
                if filtered.is_empty() {
                    crate::views::message(if list.is_empty() {
                        "No volumes yet."
                    } else {
                        "No volumes match your search."
                    })
                } else {
                    let mut rows = div().flex().flex_col();
                    for v in filtered {
                        rows = rows.child(self.row(v, cx));
                    }
                    rows.into_any_element()
                }
            }
        };

        let count = self
            .state
            .volumes
            .get(cx)
            .ready()
            .map(|l| l.len())
            .unwrap_or(0);

        let mut root = div()
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
                            .child(Text::new("Volumes").size(Size::Xl).bold())
                            .child(
                                Badge::new(count.to_string())
                                    .variant(Variant::Light)
                                    .color(ColorName::Gray)
                                    .size(Size::Xs),
                            ),
                    ),
            )
            .child(
                div()
                    .id("volumes-list")
                    .flex_1()
                    .overflow_scroll()
                    .child(body),
            );

        if let Some(name) = self.confirm_remove.clone() {
            root = root.child(
                ConfirmModal::new()
                    .title("Remove volume?")
                    .message(format!(
                        "Remove {name}? Any data stored in this volume may be permanently lost."
                    ))
                    .confirm_label("Remove volume")
                    .danger()
                    .on_confirm(cx.listener(move |this, _, _, cx| {
                        this.confirm_remove = None;
                        this.remove(name.clone(), cx);
                    }))
                    .on_cancel(cx.listener(|this, _, _, cx| this.cancel_remove(cx))),
            );
        }

        root
    }
}
