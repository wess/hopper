//! The registry view: search Docker Hub and GitHub for images, and pull them.
//!
//! Search hits the registries' own web APIs (through the host), so it works
//! whether or not an engine is up. Pull needs an engine — the image lands on
//! the Images tab when it finishes.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, Context, Entity, SharedString, Window};
use guise::prelude::*;
use model::{RegistryResult, RegistrySource};

use crate::bridge;
use crate::format;
use crate::state::{AppState, Load};
use crate::theme;

/// Per-result pull state, keyed by pullable reference.
#[derive(Clone)]
enum Pull {
    Running,
    Done,
    Failed(String),
}

pub struct Registry {
    state: AppState,
    query: Entity<TextInput>,
    source: RegistrySource,
    /// `None` until the first search; then loading / results / failure.
    results: Option<Load<Vec<RegistryResult>>>,
    pulls: HashMap<String, Pull>,
}

impl Registry {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        let query = cx.new(|cx| {
            TextInput::new(cx).placeholder("Search for an image — nginx, postgres, an owner/repo…")
        });
        // Enter runs the search against the selected source.
        cx.subscribe(&query, |this, _input, event: &TextInputEvent, cx| {
            if let TextInputEvent::Submit(text) = event {
                this.run_search(text.clone(), cx);
            }
        })
        .detach();

        Self {
            state,
            query,
            source: RegistrySource::DockerHub,
            results: None,
            pulls: HashMap::new(),
        }
    }

    fn set_source(&mut self, source: RegistrySource, cx: &mut Context<Self>) {
        if self.source == source {
            return;
        }
        self.source = source;
        // Re-run the current query against the new source, if there is one.
        let text = self.query.read(cx).text();
        if text.trim().is_empty() {
            cx.notify();
        } else {
            self.run_search(text, cx);
        }
    }

    fn run_search(&mut self, query: String, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if query.is_empty() {
            self.results = None;
            cx.notify();
            return;
        }
        self.results = Some(Load::Loading);
        cx.notify();

        let host = Arc::clone(&self.state.host);
        let source = self.source;
        let this = cx.entity().downgrade();
        bridge::run(
            cx,
            async move {
                host.registry_search(source, &query)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result, cx| {
                let Some(this) = this.upgrade() else { return };
                this.update(cx, |this, cx| {
                    this.results = Some(match result {
                        Ok(list) => Load::Ready(list),
                        Err(e) => Load::Failed(e),
                    });
                    cx.notify();
                });
            },
        );
    }

    fn pull(&mut self, reference: String, cx: &mut Context<Self>) {
        self.pulls.insert(reference.clone(), Pull::Running);
        cx.notify();

        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        let id = reference.clone();
        bridge::run(
            cx,
            // Progress frames are streamed by the daemon; for now we show a
            // simple in-flight state and report the final outcome.
            async move { host.pull(&id, &id, |_| {}).await.map_err(|e| e.message) },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        let outcome = match result {
                            Ok(t) if t.ok && t.error.is_none() => Pull::Done,
                            Ok(t) => Pull::Failed(t.error.unwrap_or_else(|| "pull failed".into())),
                            Err(e) => Pull::Failed(e),
                        };
                        this.pulls.insert(reference.clone(), outcome);
                        cx.notify();
                    });
                }
                // A freshly pulled image should show up on the Images tab.
                state.bump(cx);
            },
        );
    }

    fn source_button(&self, source: RegistrySource, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.source == source;
        Button::new(
            SharedString::from(format!("src-{}", source.as_str())),
            source.label(),
        )
        .size(Size::Xs)
        .variant(if active { Variant::Light } else { Variant::Subtle })
        .color(if active { ColorName::Blue } else { ColorName::Gray })
        .on_click(cx.listener(move |this, _, _, cx| this.set_source(source, cx)))
    }

    fn row(&self, hit: &RegistryResult, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);

        let mut meta = Group::new().gap(Size::Xs).align(Align::Center).child(
            Badge::new(hit.source.label())
                .variant(Variant::Light)
                .color(ColorName::Gray)
                .size(Size::Xs),
        );
        if hit.official {
            meta = meta.child(
                Badge::new("official")
                    .variant(Variant::Light)
                    .color(ColorName::Blue)
                    .size(Size::Xs),
            );
        }
        if hit.stars >= 0 {
            meta = meta.child(
                Text::new(format!("★ {}", format::count(hit.stars)))
                    .size(Size::Xs)
                    .dimmed(),
            );
        }

        // The pull control reflects state: idle → Pull, in flight → Pulling…,
        // done → a Pulled badge, failed → Retry.
        let action = match self.pulls.get(&hit.reference) {
            Some(Pull::Running) => Button::new(
                SharedString::from(format!("pull-{}", hit.reference)),
                "Pulling…",
            )
            .size(Size::Xs)
            .variant(Variant::Light)
            .color(ColorName::Blue)
            .disabled(true)
            .into_any_element(),
            Some(Pull::Done) => Badge::new("pulled")
                .variant(Variant::Light)
                .color(ColorName::Green)
                .size(Size::Sm)
                .into_any_element(),
            other => {
                let failed = matches!(other, Some(Pull::Failed(_)));
                let reference = hit.reference.clone();
                Button::new(
                    SharedString::from(format!("pull-{}", hit.reference)),
                    if failed { "Retry" } else { "Pull" },
                )
                .size(Size::Xs)
                .variant(Variant::Light)
                .color(if failed { ColorName::Orange } else { ColorName::Green })
                .on_click(cx.listener(move |this, _, _, cx| this.pull(reference.clone(), cx)))
                .into_any_element()
            }
        };

        let mut left = Stack::new().gap(Size::Xs).child(
            Group::new()
                .gap(Size::Xs)
                .align(Align::Center)
                .child(Text::new(hit.name.clone()).size(Size::Sm).medium())
                .child(meta),
        );
        if !hit.description.is_empty() {
            left = left.child(Text::new(hit.description.clone()).size(Size::Xs).dimmed());
        }
        // The pullable reference, so it is clear what `Pull` will fetch.
        left = left.child(Text::new(hit.reference.clone()).size(Size::Xs).dimmed());
        if let Some(Pull::Failed(e)) = self.pulls.get(&hit.reference) {
            left = left.child(Text::new(e.clone()).size(Size::Xs).dimmed());
        }

        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .p_3()
            .border_b_1()
            .border_color(palette.border_subtle)
            .child(left)
            .child(action)
    }
}

impl Render for Registry {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);

        let body = match &self.results {
            None => crate::views::message("Search Docker Hub and GitHub for images to pull."),
            Some(Load::Loading) => crate::views::message("Searching…"),
            Some(Load::Failed(e)) => crate::views::failure("Search failed", e),
            Some(Load::Ready(list)) if list.is_empty() => {
                crate::views::message("No images match your search.")
            }
            Some(Load::Ready(list)) => {
                let mut rows = div().flex().flex_col();
                for hit in list {
                    rows = rows.child(self.row(hit, cx));
                }
                rows.into_any_element()
            }
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(
                        Group::new()
                            .gap(Size::Xs)
                            .align(Align::Center)
                            .child(Text::new("Registry").size(Size::Xl).bold())
                            .child(Text::new("Find images to pull").size(Size::Xs).dimmed()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.source_button(RegistrySource::DockerHub, cx))
                            .child(self.source_button(RegistrySource::Ghcr, cx))
                            .child(div().flex_1().child(self.query.clone())),
                    ),
            )
            .child(div().flex_1().overflow_hidden().child(body))
    }
}
