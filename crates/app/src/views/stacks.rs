//! Compose stacks, reconstructed from container labels.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::{AppState, Load};
use crate::theme;
use model::{ComposeProject, ComposeStackStatus};

pub struct Stacks {
    state: AppState,
    last_epoch: u64,
    projects: Load<Vec<ComposeProject>>,
    busy: Option<String>,
}

fn status_color(status: ComposeStackStatus) -> ColorName {
    match status {
        ComposeStackStatus::Running => ColorName::Green,
        ComposeStackStatus::Partial => ColorName::Yellow,
        ComposeStackStatus::Stopped => ColorName::Gray,
    }
}

impl Stacks {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.epoch);

        let view = Self {
            state,
            last_epoch: 0,
            projects: Load::Loading,
            busy: None,
        };
        view.reload(cx);
        view
    }

    fn reload(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        cx.spawn(async move |this, cx| {
            let (tx, rx) = futures::channel::oneshot::channel();
            bridge::runtime().spawn(async move {
                let _ = tx.send(host.compose_projects().await);
            });
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |this: &mut Self, cx| {
                    this.projects = match result {
                        Ok(list) => Load::Ready(list),
                        Err(e) => Load::Failed(e.message),
                    };
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Apply a lifecycle action to every container in a stack.
    ///
    /// This works label-driven through the Docker API rather than shelling out
    /// to Compose, so a stack still starts and stops when its compose file has
    /// moved or been deleted.
    fn act(&mut self, project: String, start: bool, cx: &mut Context<Self>) {
        let Load::Ready(list) = &self.projects else {
            return;
        };
        let Some(stack) = list.iter().find(|p| p.name == project) else {
            return;
        };
        let ids: Vec<String> = stack
            .services
            .iter()
            .map(|s| s.container_id.clone())
            .collect();

        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        self.busy = Some(project);
        cx.notify();

        bridge::run(
            cx,
            async move {
                let action = if start {
                    host::facade::BatchAction::Start
                } else {
                    host::facade::BatchAction::Stop
                };
                host.container_batch(&ids, action).await
            },
            move |results, cx| {
                for (id, error) in results {
                    if let Some(e) = error {
                        // One failed service must not hide the rest.
                        tracing::warn!("stack action failed for {id}: {e}");
                    }
                }
                state.bump(cx);
            },
        );
    }

    fn row(&self, p: &ComposeProject, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let busy = self.busy.as_deref() == Some(p.name.as_str());
        let up = p.running > 0;
        let name_toggle = p.name.clone();

        let services = p
            .service_names()
            .into_iter()
            .take(4)
            .collect::<Vec<_>>()
            .join(", ");
        let extra = p.service_names().len().saturating_sub(4);

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
                            .child(Text::new(p.name.clone()).size(Size::Sm).medium())
                            .child(
                                Badge::new(format!("{}/{}", p.running, p.total))
                                    .variant(Variant::Light)
                                    .color(status_color(p.status))
                                    .size(Size::Xs),
                            ),
                    )
                    .child(
                        Text::new(if extra > 0 {
                            format!("{services} +{extra} more")
                        } else {
                            services
                        })
                        .size(Size::Xs)
                        .dimmed(),
                    ),
            )
            .child(
                Button::new(
                    SharedString::from(format!("stack-{}", p.name)),
                    if up { "Stop" } else { "Start" },
                )
                .size(Size::Xs)
                .variant(Variant::Light)
                .color(if up { ColorName::Red } else { ColorName::Green })
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.act(name_toggle.clone(), !up, cx);
                })),
            )
    }
}

impl Render for Stacks {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.busy = None;
            self.reload(cx);
        }

        let palette = theme::palette(cx);
        let body = match self.projects.clone() {
            Load::Loading => crate::views::message("Loading stacks…"),
            Load::Failed(e) => crate::views::failure("Could not list stacks", &e),
            Load::Ready(list) if list.is_empty() => crate::views::message(
                "No compose stacks. Containers started by Docker Compose appear here.",
            ),
            Load::Ready(list) => {
                let mut rows = div().flex().flex_col();
                for p in &list {
                    rows = rows.child(self.row(p, cx));
                }
                rows.into_any_element()
            }
        };

        let count = self.projects.ready().map(|l| l.len()).unwrap_or(0);

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
                            .child(Text::new("Stacks").size(Size::Xl).bold())
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
