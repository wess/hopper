//! The images list.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::format;
use crate::state::{AppState, Load};
use crate::theme;
use model::Image;

pub struct Images {
    state: AppState,
    last_epoch: u64,
    busy: Option<String>,
}

impl Images {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.images);
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
        let signal = self.state.images.clone();
        bridge::run(
            cx,
            async move { host.images(false).await },
            move |result, cx| {
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

    fn remove(&mut self, id: String, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = Some(id.clone());
        cx.notify();
        bridge::run(
            cx,
            async move { host.image_remove(&id, false).await },
            move |result, cx| {
                if let Err(e) = result {
                    // In use by a container is the common case; the daemon's
                    // own message says which one.
                    tracing::warn!("image remove failed: {}", e.message);
                }
                state.bump(cx);
            },
        );
    }

    fn row(&self, img: &Image, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let busy = self.busy.as_deref() == Some(img.id.as_str());
        let id = img.id.clone();
        // What `docker run` should target: a real tag if there is one, else the
        // image id (an untagged image can only be run by id).
        let run_ref = img
            .repo_tags
            .iter()
            .find(|t| *t != "<none>:<none>")
            .cloned()
            .unwrap_or_else(|| img.id.clone());

        let mut badges = Group::new().gap(Size::Xs);
        if img.dangling {
            badges = badges.child(
                Badge::new("dangling")
                    .variant(Variant::Light)
                    .color(ColorName::Orange)
                    .size(Size::Xs),
            );
        }
        if img.containers > 0 {
            badges = badges.child(
                Badge::new(format!("{} in use", img.containers))
                    .variant(Variant::Light)
                    .color(ColorName::Blue)
                    .size(Size::Xs),
            );
        }

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
                            .child(Text::new(img.display_name()).size(Size::Sm).medium())
                            .child(badges),
                    )
                    .child(
                        Text::new(format!(
                            "{}  ·  {}  ·  {}",
                            img.short_id(),
                            format::bytes(img.size),
                            format::ago(img.created)
                        ))
                        .size(Size::Xs)
                        .dimmed(),
                    ),
            )
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .child(
                        Button::new(SharedString::from(format!("run-{}", img.id)), "Run")
                            .size(Size::Xs)
                            .variant(Variant::Light)
                            .color(ColorName::Green)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.run_image(cx, run_ref.clone());
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("rm-{}", img.id)), "Remove")
                            .size(Size::Xs)
                            .variant(Variant::Subtle)
                            .color(ColorName::Red)
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| this.remove(id.clone(), cx))),
                    ),
            )
    }
}

impl Render for Images {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.busy = None;
            self.reload(cx);
        }

        let palette = theme::palette(cx);
        let load = self.state.images.get(cx);
        let query = self.state.search.get(cx);

        let body = match load {
            Load::Loading => crate::views::message("Loading images…"),
            Load::Failed(e) => crate::views::failure("Could not list images", &e),
            Load::Ready(list) => {
                let q = query.trim().to_lowercase();
                let filtered: Vec<&Image> = list
                    .iter()
                    .filter(|i| {
                        q.is_empty()
                            || i.repo_tags.iter().any(|t| t.to_lowercase().contains(&q))
                            || i.id.contains(&q)
                    })
                    .collect();
                if filtered.is_empty() {
                    if list.is_empty() {
                        crate::views::empty_cta(
                            &self.state,
                            "No images yet",
                            "Search the Registry to find an image, then pull it here.",
                            "Browse the Registry",
                            crate::state::Route::Registry,
                            cx,
                        )
                    } else {
                        crate::views::message("No images match your search.")
                    }
                } else {
                    let mut rows = div().flex().flex_col();
                    for img in filtered {
                        rows = rows.child(self.row(img, cx));
                    }
                    rows.into_any_element()
                }
            }
        };

        let count = self
            .state
            .images
            .get(cx)
            .ready()
            .map(|l| l.len())
            .unwrap_or(0);
        let total: i64 = self
            .state
            .images
            .get(cx)
            .ready()
            .map(|l| l.iter().map(|i| i.size).sum())
            .unwrap_or(0);

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
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
                            .child(Text::new("Images").size(Size::Xl).bold())
                            .child(
                                Badge::new(count.to_string())
                                    .variant(Variant::Light)
                                    .color(ColorName::Gray)
                                    .size(Size::Xs),
                            ),
                    )
                    .child(Text::new(format::bytes(total)).size(Size::Xs).dimmed()),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}
