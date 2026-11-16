//! Coexistence, migration and explicit removal of other local container tools.
use crate::{
    bridge,
    state::{AppState, Route},
    theme,
};
use gpui::prelude::*;
use gpui::{div, px, Context, Entity, SharedString, Task, Window};
use guise::prelude::*;
use model::{EngineInstallation, RemovalConsent, RemovalReview};
use std::sync::Arc;

pub struct Replacement {
    state: AppState,
    installations: Vec<EngineInstallation>,
    busy: bool,
    loaded: bool,
    notice: Option<String>,
    review: Option<RemovalReview>,
    name: Entity<TextArea>,
    backups: bool,
    workloads: bool,
    task: Option<Task<()>>,
}

impl Replacement {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.engine);
        let name = cx.new(|cx| {
            TextArea::new(cx)
                .label("Type the application name to confirm")
                .rows(1)
                .max_rows(1)
                .submit_on_enter(true)
        });
        cx.observe(&name, |_, _, cx| cx.notify()).detach();
        let mut view = Self {
            state,
            installations: Vec::new(),
            busy: false,
            loaded: false,
            notice: None,
            review: None,
            name,
            backups: false,
            workloads: false,
            task: None,
        };
        view.refresh(cx);
        view
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.review = None;
        self.busy = true;
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();
        self.task = Some(bridge::run_owned(
            cx,
            async move { host.engine_installations().await },
            move |items, cx| {
                let _ = entity.update(cx, |this, cx| {
                    this.installations = items;
                    this.loaded = true;
                    this.busy = false;
                    cx.notify();
                });
            },
        ));
        cx.notify();
    }

    fn use_existing(&mut self, provider: String, cx: &mut Context<Self>) {
        self.busy = true;
        self.review = None;
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();
        self.task = Some(bridge::run_owned(
            cx,
            async move { host.set_engine_preference(Some(provider)).await },
            move |result, cx| {
                let _ = entity.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(status) => { this.state.engine.set(cx, status); this.state.settings.set(cx, this.state.host.settings()); this.state.bump(cx); this.notice = Some("Selection saved. Start this engine in its own application if it is stopped.".into()); },
                    Err(error) => this.notice = Some(error),
                }
                cx.notify();
            });
            },
        ));
        cx.notify();
    }

    fn use_hopper(&mut self, source: Option<String>, cx: &mut Context<Self>) {
        self.busy = true;
        self.review = None;
        self.notice = Some("Connecting Hopper Engine…".into());
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();
        self.task = Some(bridge::run_owned(
            cx,
            async move {
                let status = host.set_engine_preference(Some("vm".into())).await?;
                if status.provider != "vm" {
                    return Err("An environment override selected a different engine. Remove the override before replacing it with Hopper Engine.".into());
                }
                if !status.connected {
                    host.start_engine().await.map_err(|e| e.to_string())?;
                }
                let status = host.engine_status().await;
                if status.provider != "vm" {
                    return Err("Engine selection changed during startup. Review the active engine before continuing.".into());
                }
                if !status.connected {
                    return Err(status.message);
                }
                Ok(status)
            },
            move |result: Result<model::EngineStatus, String>, cx| {
                let _ = entity.update(cx, |this, cx| {
                this.busy = false;
                this.state.settings.set(cx, this.state.host.settings());
                match result {
                    Ok(status) => {
                        this.state.engine.set(cx, status); this.state.bump(cx);
                        this.notice = Some("Hopper Engine is connected. The other application remains installed. Stop it in its own app when it is no longer needed to release its resources.".into());
                        if let Some(provider) = source { this.state.import_provider.set(cx, Some(provider)); this.state.route.set(cx, Route::Import); }
                    },
                    Err(error) => this.notice = Some(error),
                }
                cx.notify();
            });
            },
        ));
        cx.notify();
    }

    fn review(&mut self, id: String, cx: &mut Context<Self>) {
        self.busy = true;
        self.review = None;
        self.backups = false;
        self.workloads = false;
        self.name.update(cx, |input, cx| input.set_text("", cx));
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();
        self.task = Some(bridge::run_owned(
            cx,
            async move { host.removal_review(&id).await },
            move |result, cx| {
                let _ = entity.update(cx, |this, cx| {
                    this.busy = false;
                    match result {
                        Ok(review) => this.review = Some(review),
                        Err(error) => this.notice = Some(error),
                    }
                    cx.notify();
                });
            },
        ));
        cx.notify();
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(review) = &self.review else {
            return;
        };
        let consent = RemovalConsent {
            application_name: self.name.read(cx).text(),
            backups_verified: self.backups,
            workloads_verified: self.workloads,
        };
        if host::replacement::consent(&review.installation, &consent).is_err()
            || !review.blockers.is_empty()
        {
            return;
        }
        let id = review.installation.id.clone();
        self.busy = true;
        let host = Arc::clone(&self.state.host);
        let entity = cx.entity().downgrade();
        self.task = Some(bridge::run_owned(
            cx,
            async move { host.launch_engine_removal(&id, &consent).await },
            move |result, cx| {
                let _ = entity.update(cx, |this, cx| {
                    this.busy = false;
                    this.review = None;
                    this.notice = Some(result.unwrap_or_else(|error| error));
                    cx.notify();
                });
            },
        ));
        cx.notify();
    }
}

impl Render for Replacement {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let mut body = div().w_full().min_w_0().flex().flex_col().gap_3()
            .child(Group::new().gap(Size::Sm).wrap(true)
                .child(Text::new("Other container tools").size(Size::Sm).bold())
                .child(Button::new("refresh-installations", if self.busy { "Working…" } else { "Refresh detection" }).size(Size::Xs).variant(Variant::Subtle).disabled(self.busy).on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))))
            .child(Text::new("Keep an existing engine, run Hopper alongside it, or replace it after importing and independently backing up its data. Running two engines uses extra memory and can cause published-port conflicts.").size(Size::Xs).dimmed());
        if self.loaded && self.installations.is_empty() {
            body = body.child(Text::new(if cfg!(target_os = "macos") { "No other local container tools were detected." } else { "Hopper needs an installed Docker-compatible engine on this platform. Keep one available; use your system application manager for redundant desktop applications." }).size(Size::Xs));
        }
        for installation in &self.installations {
            let provider = installation.provider.clone();
            let source = provider.clone();
            let id = installation.id.clone();
            body = body.child(
                div()
                    .p_3()
                    .border_1()
                    .border_color(palette.border_subtle)
                    .rounded_md()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        Text::new(format!("{} · installed", installation.label))
                            .size(Size::Xs)
                            .bold(),
                    )
                    .child(
                        Text::new(installation.location.clone())
                            .size(Size::Xs)
                            .dimmed(),
                    )
                    .child(
                        Group::new()
                            .gap(Size::Xs)
                            .wrap(true)
                            .child(
                                Button::new(
                                    SharedString::from(format!("keep-{}", installation.id)),
                                    "Keep using this engine",
                                )
                                .size(Size::Xs)
                                .variant(Variant::Light)
                                .disabled(self.busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.use_existing(provider.clone(), cx),
                                )),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("replace-{}", installation.id)),
                                    "Use Hopper instead",
                                )
                                .size(Size::Xs)
                                .variant(Variant::Light)
                                .disabled(
                                    self.busy
                                        || !cfg!(all(target_os = "macos", target_arch = "aarch64")),
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.use_hopper(None, cx))),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("import-{}", installation.id)),
                                    "Import workloads",
                                )
                                .size(Size::Xs)
                                .variant(Variant::Subtle)
                                .disabled(
                                    self.busy
                                        || !cfg!(all(target_os = "macos", target_arch = "aarch64")),
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.use_hopper(Some(source.clone()), cx),
                                )),
                            )
                            .child(
                                Button::new(
                                    SharedString::from(format!("review-{}", installation.id)),
                                    "Review removal…",
                                )
                                .size(Size::Xs)
                                .variant(Variant::Subtle)
                                .color(ColorName::Red)
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.review(id.clone(), cx)),
                                ),
                            ),
                    ),
            );
        }
        if let Some(review) = self.review.clone() {
            let url = review.installation.instructions.clone();
            let approved = host::replacement::consent(
                &review.installation,
                &RemovalConsent {
                    application_name: self.name.read(cx).text(),
                    backups_verified: self.backups,
                    workloads_verified: self.workloads,
                },
            )
            .is_ok();
            let mut panel = div()
                .w_full()
                .min_w_0()
                .p_3()
                .border_1()
                .border_color(palette.border)
                .rounded_md()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    Text::new(format!("Remove {}", review.installation.label))
                        .size(Size::Sm)
                        .bold(),
                )
                .child(Text::new(review.consequences).size(Size::Xs))
                .child(
                    Button::new("vendor-removal-guide", "Official removal instructions")
                        .size(Size::Xs)
                        .variant(Variant::Subtle)
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                );
            for blocker in &review.blockers {
                panel = panel.child(Text::new(blocker.clone()).size(Size::Xs));
            }
            if let Some(command) = review.command {
                panel = panel
                    .child(
                        Text::new(format!("Command reviewed in Terminal: {command}"))
                            .size(Size::Xs)
                            .dimmed(),
                    )
                    .child(
                        Checkbox::new("removal-backup")
                            .checked(self.backups)
                            .label("I have verified independent backups of the source data.")
                            .on_change(cx.listener(|this, _, _, cx| {
                                this.backups = !this.backups;
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new("removal-workloads")
                            .checked(self.workloads)
                            .label("I verified my workloads in Hopper and stopped the old engine.")
                            .on_change(cx.listener(|this, _, _, cx| {
                                this.workloads = !this.workloads;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .h(px(76.0))
                            .flex()
                            .flex_col()
                            .child(self.name.clone()),
                    )
                    .child(
                        Button::new("launch-removal", "Review and run removal in Terminal…")
                            .size(Size::Xs)
                            .color(ColorName::Red)
                            .disabled(self.busy || !approved || !review.blockers.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.remove(cx))),
                    );
            }
            panel = panel.child(
                Button::new("cancel-removal", "Cancel removal review")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.review = None;
                        cx.notify();
                    })),
            );
            body = body.child(panel);
        }
        if let Some(notice) = &self.notice {
            body = body.child(Text::new(notice.clone()).size(Size::Xs));
        }
        body
    }
}
