//! The dashboard: engine health, resource counts, and disk usage.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, Window};
use guise::prelude::*;

use crate::bridge;
use crate::format;
use crate::state::AppState;
use crate::theme;
use model::{DiskUsage, SystemInfo};

pub struct Dashboard {
    state: AppState,
    last_epoch: u64,
    info: Option<SystemInfo>,
    usage: Option<DiskUsage>,
    error: Option<String>,
    pruning: bool,
}

impl Dashboard {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.epoch);
        watch(cx, &state.engine);

        let view = Self {
            state,
            last_epoch: 0,
            info: None,
            usage: None,
            error: None,
            pruning: false,
        };
        view.reload(cx);
        view
    }

    fn reload(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        cx.spawn(async move |this, cx| {
            let (tx, rx) = futures::channel::oneshot::channel();
            bridge::runtime().spawn(async move {
                let info = host.info().await;
                let usage = host.disk_usage().await;
                let _ = tx.send((info, usage));
            });
            if let Ok((info, usage)) = rx.await {
                let _ = this.update(cx, |this: &mut Self, cx| {
                    match info {
                        Ok(i) => {
                            this.info = Some(i);
                            this.error = None;
                        }
                        Err(e) => this.error = Some(e.message),
                    }
                    this.usage = usage.ok();
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn prune(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.pruning = true;
        cx.notify();
        bridge::run(cx, async move { host.prune_all().await }, move |reports, cx| {
            let freed: i64 = reports.iter().map(|r| r.reclaimed).sum();
            tracing::info!("cleanup reclaimed {}", format::bytes(freed));
            state.bump(cx);
        });
    }

    fn stat(&self, label: &str, value: String, hint: Option<String>, cx: &gpui::App) -> impl IntoElement {
        let palette = theme::palette(cx);
        let mut stack = Stack::new()
            .gap(Size::Xs)
            .child(Text::new(value).size(Size::Xl).bold())
            .child(Text::new(label.to_string()).size(Size::Xs).dimmed());
        if let Some(hint) = hint {
            stack = stack.child(Text::new(hint).size(Size::Xs).dimmed());
        }
        div()
            .flex_1()
            .p_4()
            .rounded_md()
            .bg(palette.bg_subtle)
            .border_1()
            .border_color(palette.border_subtle)
            .child(stack)
    }
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.pruning = false;
            self.reload(cx);
        }

        let palette = theme::palette(cx);
        let engine = self.state.engine.get(cx);

        let body = if let Some(err) = self.error.clone() {
            crate::views::failure("Could not reach the Docker engine", &err)
        } else if let Some(info) = self.info.clone() {
            let usage = self.usage;
            let reclaimable = usage.map(|u| u.total_reclaimable()).unwrap_or(0);

            let counts = div()
                .flex()
                .gap_3()
                .child(self.stat(
                    "Containers running",
                    info.containers_running.to_string(),
                    Some(format!("{} total", info.containers)),
                    cx,
                ))
                .child(self.stat("Images", info.images.to_string(), None, cx))
                .child(self.stat(
                    "CPUs",
                    info.ncpu.to_string(),
                    Some(format::bytes(info.mem_total)),
                    cx,
                ));

            let disk = usage.map(|u| {
                div()
                    .flex()
                    .gap_3()
                    .child(self.stat("Images on disk", format::bytes(u.images.size), None, cx))
                    .child(self.stat("Volumes", format::bytes(u.volumes.size), None, cx))
                    .child(self.stat(
                        "Build cache",
                        format::bytes(u.build_cache.size),
                        None,
                        cx,
                    ))
            });

            let cleanup = Button::new(
                "cleanup",
                if self.pruning {
                    "Cleaning up…".to_string()
                } else {
                    format!("Clean up ({} reclaimable)", format::bytes(reclaimable))
                },
            )
            .size(Size::Sm)
            .variant(Variant::Light)
            .color(ColorName::Blue)
            // Nothing to reclaim means nothing to do; a live button would just
            // report "freed 0 B".
            .disabled(self.pruning || reclaimable <= 0)
            .on_click(cx.listener(|this, _, _, cx| this.prune(cx)));

            let mut stack = Stack::new()
                .gap(Size::Md)
                .child(Text::new(info.name.clone()).size(Size::Sm).medium())
                .child(counts);
            if let Some(disk) = disk {
                stack = stack.child(disk).child(cleanup);
            }
            div().p_4().child(stack).into_any_element()
        } else {
            crate::views::message("Loading…")
        };

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
                    .child(Text::new("Dashboard").size(Size::Xl).bold())
                    .child(
                        Badge::new(engine.state.as_str())
                            .variant(Variant::Light)
                            .color(if engine.connected {
                                ColorName::Green
                            } else {
                                ColorName::Gray
                            })
                            .size(Size::Xs),
                    ),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}
