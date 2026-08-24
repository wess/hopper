//! Compose stacks: the ones already running, and bringing new ones up.
//!
//! The list is reconstructed from container labels, so a stack started by the
//! real `docker compose` shows up here unchanged. Bringing one up goes the
//! other way — Hopper reads the file itself and creates the containers, which
//! is what makes this work on a Mac with no Docker at all.
//!
//! A run streams its lines into the panel rather than reporting only at the
//! end. Pulling three images takes long enough that silence reads as a hang.

use std::collections::BTreeSet;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, Context, PathPromptOptions, SharedString, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::{AppState, Load};
use crate::theme;
use model::{ComposeProject, ComposeProgress, ComposeStackStatus, StreamKind};

/// A compose run in flight, or the one that just finished.
struct Run {
    project: String,
    /// `(text, is_problem)` — warnings and failures are coloured, not hidden.
    lines: Vec<(SharedString, bool)>,
    done: bool,
    failed: bool,
}

pub struct Stacks {
    state: AppState,
    last_epoch: u64,
    projects: Load<Vec<ComposeProject>>,
    /// Stacks whose compose file is still on disk, so `up` has something to
    /// read. Settled when the list loads: probing the filesystem from `row`
    /// would stat every file on every frame.
    startable: BTreeSet<String>,
    busy: Option<String>,
    run: Option<Run>,
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
            startable: BTreeSet::new(),
            busy: None,
            run: None,
        };
        view.reload(cx);
        view
    }

    fn reload(&self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        bridge::run(
            cx,
            async move { host.compose_projects().await },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.projects = match result {
                            Ok(list) => {
                                this.startable = list
                                    .iter()
                                    .filter(|p| host::stacks::can_start_from_files(p))
                                    .map(|p| p.name.clone())
                                    .collect();
                                Load::Ready(list)
                            }
                            Err(e) => Load::Failed(e.message),
                        };
                        cx.notify();
                    });
                }
            },
        );
    }

    /// Start or stop the containers a stack already has.
    ///
    /// Label-driven rather than file-driven, so a stack still starts and stops
    /// when its compose file has moved or been deleted.
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

    /// Ask macOS for a compose file, then bring it up.
    fn open(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Bring up".into()),
        });
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(picked))) = paths.await else {
                return;
            };
            let Some(path) = picked.first().map(|p| p.to_string_lossy().to_string()) else {
                return;
            };
            let _ = cx.update(|cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| this.up_from_path(path, cx));
                }
            });
        })
        .detach();
    }

    /// Plan a path — a compose file, or a directory holding one — and run it.
    fn up_from_path(&mut self, path: String, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        self.stream_up(
            move || host.compose_plan_path(&path, &compose::PlanOptions::default()),
            cx,
        );
    }

    /// Bring a stack that is already here back up, from the files its
    /// containers remember.
    ///
    /// Looked up by name on click rather than captured by the row, so a render
    /// does not deep-clone every project into a listener it may never fire.
    fn up_from_project(&mut self, name: String, cx: &mut Context<Self>) {
        let Load::Ready(list) = &self.projects else {
            return;
        };
        let Some(project) = list.iter().find(|p| p.name == name).cloned() else {
            return;
        };
        let host = Arc::clone(&self.state.host);
        self.stream_up(
            move || host.compose_plan_project(&project, &compose::PlanOptions::default()),
            cx,
        );
    }

    /// Plan on the tokio side, then run, streaming every line into the panel.
    fn stream_up(
        &mut self,
        plan: impl FnOnce() -> Result<model::ComposePlan, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.run = Some(Run {
            project: "Reading the compose file…".into(),
            lines: Vec::new(),
            done: false,
            failed: false,
        });
        cx.notify();

        bridge::stream(
            cx,
            move |tx| async move {
                let plan = match plan() {
                    Ok(plan) => plan,
                    Err(reason) => {
                        // A file that will not parse is the end of the run, and
                        // the reason is the only thing worth showing.
                        let _ = tx.unbounded_send(ComposeProgress {
                            request_id: String::new(),
                            line: reason.clone(),
                            stream: StreamKind::Stderr,
                            done: true,
                            error: Some(reason),
                        });
                        return;
                    }
                };
                let mut sink = move |p: ComposeProgress| {
                    let _ = tx.unbounded_send(p);
                };
                host.compose_up(&plan, &mut sink).await;
            },
            {
                let this = this.clone();
                move |progress: ComposeProgress, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            let run = this.run.get_or_insert_with(|| Run {
                                project: progress.request_id.clone(),
                                lines: Vec::new(),
                                done: false,
                                failed: false,
                            });
                            if !progress.request_id.is_empty() {
                                run.project = progress.request_id.clone();
                            }
                            run.lines.push((
                                progress.line.clone().into(),
                                progress.stream == StreamKind::Stderr,
                            ));
                            if progress.done {
                                run.done = true;
                                run.failed = progress.error.is_some();
                            }
                            cx.notify();
                        });
                    }
                }
            },
            move |cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if let Some(run) = &mut this.run {
                            run.done = true;
                        }
                        cx.notify();
                    });
                }
                // Whatever came up has to appear in every other list too.
                state.bump(cx);
            },
        );
    }

    /// Stop and remove a stack's containers and the networks it owns.
    fn down(&mut self, project: String, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.busy = Some(project.clone());
        self.run = Some(Run {
            project: project.clone(),
            lines: Vec::new(),
            done: false,
            failed: false,
        });
        cx.notify();

        bridge::stream(
            cx,
            move |tx| async move {
                let mut sink = move |p: ComposeProgress| {
                    let _ = tx.unbounded_send(p);
                };
                host.compose_down(&project, false, &mut sink).await;
            },
            {
                let this = this.clone();
                move |progress: ComposeProgress, cx| {
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| {
                            if let Some(run) = &mut this.run {
                                run.lines.push((
                                    progress.line.clone().into(),
                                    progress.stream == StreamKind::Stderr,
                                ));
                                if progress.done {
                                    run.done = true;
                                    run.failed = progress.error.is_some();
                                }
                            }
                            cx.notify();
                        });
                    }
                }
            },
            move |cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.busy = None;
                        if let Some(run) = &mut this.run {
                            run.done = true;
                        }
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }

    /// The run panel: what happened, in order, with problems coloured.
    fn panel(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let t = guise::theme::theme(cx);
        let red = t.color(ColorName::Red, 6);

        let mut lines = Stack::new().gap(Size::Xs);
        // The tail is what matters while a long run is in flight, and a big
        // stack must not push the list off the screen.
        const KEEP: usize = 14;
        let start = run.lines.len().saturating_sub(KEEP);
        for (text, problem) in &run.lines[start..] {
            let mut line = Text::new(text.clone()).size(Size::Xs);
            if *problem {
                line = line.color(red);
            }
            lines = lines.child(line);
        }

        let head = Group::new()
            .gap(Size::Xs)
            .align(Align::Center)
            .child(if run.done {
                Icon::new(if run.failed {
                    IconName::TriangleAlert
                } else {
                    IconName::Check
                })
                .size(Size::Sm)
                .color(if run.failed {
                    ColorName::Red
                } else {
                    ColorName::Green
                })
                .into_any_element()
            } else {
                Loader::new()
                    .size(Size::Sm)
                    .color(ColorName::Blue)
                    .into_any_element()
            })
            .child(Text::new(run.project.clone()).size(Size::Sm).medium())
            .child(
                Button::new("stack-run-dismiss", "Dismiss")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .color(ColorName::Gray)
                    .disabled(!run.done)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.run = None;
                        cx.notify();
                    })),
            );

        div()
            .p_3()
            .border_b_1()
            .border_color(palette.border_subtle)
            .bg(palette.bg_subtle)
            .child(
                Stack::new().gap(Size::Sm).child(head).child(
                    div()
                        .max_h(px(220.0))
                        .overflow_hidden()
                        .child(lines),
                ),
            )
    }

    fn row(&self, p: &ComposeProject, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let busy = self.busy.as_deref() == Some(p.name.as_str());
        let up = p.running > 0;
        let name_toggle = p.name.clone();
        let name_down = p.name.clone();
        let name_up = p.name.clone();
        // `up` needs the file the stack came from; a stack whose file has been
        // moved or deleted can still be stopped and removed.
        let has_files = self.startable.contains(&p.name);

        let services = p
            .service_names()
            .into_iter()
            .take(4)
            .collect::<Vec<_>>()
            .join(", ");
        let extra = p.service_names().len().saturating_sub(4);

        let mut actions = Group::new().gap(Size::Xs);
        if has_files {
            actions = actions.child(
                Button::new(SharedString::from(format!("stack-up-{}", p.name)), "Up")
                    .size(Size::Xs)
                    .variant(Variant::Light)
                    .color(ColorName::Blue)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.up_from_project(name_up.clone(), cx)
                    })),
            );
        }
        actions = actions
            .child(
                Button::new(
                    SharedString::from(format!("stack-{}", p.name)),
                    if up { "Stop" } else { "Start" },
                )
                .size(Size::Xs)
                .variant(Variant::Subtle)
                .color(if up { ColorName::Yellow } else { ColorName::Green })
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.act(name_toggle.clone(), !up, cx);
                })),
            )
            .child(
                Button::new(SharedString::from(format!("stack-down-{}", p.name)), "Down")
                    .size(Size::Xs)
                    .variant(Variant::Subtle)
                    .color(ColorName::Red)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.down(name_down.clone(), cx)
                    })),
            );

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
            .child(actions)
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
                "No stacks yet. Open a compose file and Hopper will bring it up — \
                 it reads the file itself, so no Docker or Compose is needed.",
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
        let mut content = div().flex().flex_col().size_full();

        content = content.child(
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
                        .child(Text::new("Stacks").size(Size::Xl).bold())
                        .child(
                            Badge::new(count.to_string())
                                .variant(Variant::Light)
                                .color(ColorName::Gray)
                                .size(Size::Xs),
                        ),
                )
                .child(
                    Button::new("stack-open", "Open compose file…")
                        .size(Size::Xs)
                        .variant(Variant::Light)
                        .color(ColorName::Blue)
                        .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                ),
        );

        if let Some(run) = &self.run {
            content = content.child(self.panel(run, cx));
        }

        content.child(div().flex_1().overflow_hidden().child(body))
    }
}
