//! Settings: engine choice and control, CLI integration, and appearance.
//!
//! Lifecycle and backend resource guidance live here. Apple sizes each
//! container's VM when it runs it, so its per-container limits are explained
//! instead of presenting global controls that do nothing.
//!
//! The engine picker is the one place a person moving off Docker Desktop can
//! stand in both worlds: keep pointing at Docker while images come across,
//! then pin Apple's runtime — or install it from here without first having to
//! turn Docker off to be shown the offer.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{div, px, Context, Entity, SharedString, UpdateGlobal, Window};
use guise::prelude::*;

use crate::bridge;
use crate::state::AppState;
use crate::theme;
use crate::views::engine::provider_label;
use model::{EngineChoice, EngineState, RuntimeKind, ThemeMode};

fn engine_state_label(state: EngineState) -> &'static str {
    match state {
        EngineState::Connected => "Connected",
        EngineState::Starting => "Starting",
        EngineState::Stopped => "Stopped",
        EngineState::NotInstalled => "Not installed",
        EngineState::Unreachable => "Not responding",
        EngineState::NeedsPermission => "Needs permission",
        EngineState::Unsupported => "Unsupported",
    }
}

fn engine_state_color(state: EngineState) -> ColorName {
    match state {
        EngineState::Connected => ColorName::Green,
        EngineState::Starting => ColorName::Blue,
        EngineState::NeedsPermission | EngineState::Unreachable | EngineState::Unsupported => {
            ColorName::Orange
        }
        EngineState::Stopped | EngineState::NotInstalled => ColorName::Gray,
    }
}

fn choice_status(choice: &EngineChoice) -> (&'static str, ColorName) {
    if choice.connected || choice.state == EngineState::Connected {
        return ("Connected", ColorName::Green);
    }
    if choice.id != "existing" && choice.installed && choice.state == EngineState::NotInstalled {
        return ("Installed", ColorName::Gray);
    }
    match choice.state {
        EngineState::Connected => ("Connected", ColorName::Green),
        EngineState::Starting => ("Starting", ColorName::Blue),
        EngineState::Stopped => ("Stopped", ColorName::Gray),
        EngineState::Unreachable => ("Not responding", ColorName::Orange),
        EngineState::NeedsPermission => ("Needs permission", ColorName::Orange),
        EngineState::Unsupported => ("Unsupported", ColorName::Orange),
        EngineState::NotInstalled => {
            // `available` means a provider can be addressed, not that its
            // endpoint answered. Keep the distinction visible for a named
            // provider whose socket exists but is currently idle.
            if choice.available && choice.id != "existing" {
                ("Available", ColorName::Blue)
            } else {
                ("Not installed", ColorName::Gray)
            }
        }
    }
}

pub struct Settings {
    state: AppState,
    replacement: Entity<super::replacement::Replacement>,
    busy: bool,
    lifecycle_generation: u64,
    notice: Option<String>,
    resource_notice: Option<Result<String, String>>,
    cli_notice: Option<Result<String, String>>,
    cpus: Entity<NumberInput>,
    memory: Entity<NumberInput>,
    disk: Entity<NumberInput>,
    share_path: Entity<TextArea>,
    share_writable: bool,
    daemon_json: Entity<TextArea>,
    vm_notice: Option<String>,
    vm_stats: Option<model::EngineStats>,
    /// Engines this machine could be pointed at.
    choices: Vec<EngineChoice>,
    /// The pinned engine id, or `None` for automatic.
    preference: Option<String>,
    /// A provider probe is in flight; keep the picker honest about freshness.
    loading_choices: bool,
    /// Monotonically identifies the newest picker probe. Older responses must
    /// not replace a snapshot taken after an engine switch or refresh.
    choices_request: u64,
    /// An Apple Containers install is downloading; macOS takes over after.
    #[cfg(target_os = "macos")]
    installing: bool,
}

impl Settings {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.engine);
        watch(cx, &state.settings);
        let settings = state.host.settings();
        let capacity = state.host.engine_capacity();
        let preference = settings.engine_preference.clone();
        let cpus = cx.new(|cx| {
            NumberInput::new(cx)
                .min(1.0)
                .max(capacity.max_cpus as f64)
                .step(1.0)
                .value(settings.resources.cpus as f64)
                .label("CPU cores")
        });
        let memory = cx.new(|cx| {
            NumberInput::new(cx)
                .min(1.0)
                .max(capacity.max_memory_gib as f64)
                .step(1.0)
                .value(settings.resources.memory_gib as f64)
                .label("Memory (GiB)")
        });
        let disk = cx.new(|cx| {
            NumberInput::new(cx)
                .min(10.0)
                .max(2048.0)
                .step(1.0)
                .value(settings.resources.disk_gib as f64)
                .label("Disk (GiB)")
        });
        let share_path = cx.new(|cx| {
            TextArea::new(cx)
                .label("Directory to share")
                .placeholder("Absolute host directory")
                .rows(1)
                .max_rows(1)
                .submit_on_enter(true)
        });
        let daemon_json = cx.new(|cx| {
            let mut input = TextArea::new(cx)
                .label("Docker daemon JSON")
                .rows(8)
                .max_rows(8);
            input.set_text(
                &state.host.daemon_document().unwrap_or_else(|_| "{}".into()),
                cx,
            );
            input
        });
        let replacement = cx.new(super::replacement::Replacement::new);
        let mut view = Self {
            state,
            replacement,
            busy: false,
            lifecycle_generation: 0,
            notice: None,
            resource_notice: None,
            cli_notice: None,
            cpus,
            memory,
            disk,
            share_path,
            share_writable: false,
            daemon_json,
            vm_notice: None,
            vm_stats: None,
            choices: Vec::new(),
            preference,
            loading_choices: false,
            choices_request: 0,
            #[cfg(target_os = "macos")]
            installing: false,
        };
        view.load_choices(cx);
        view
    }

    fn reload_saved(&mut self, cx: &mut Context<Self>) {
        self.state.host.reload_preferences();
        let settings = self.state.host.settings();
        self.cpus.update(cx, |input, cx| {
            input.set_value(settings.resources.cpus as f64, cx)
        });
        self.memory.update(cx, |input, cx| {
            input.set_value(settings.resources.memory_gib as f64, cx)
        });
        self.disk.update(cx, |input, cx| {
            input.set_value(settings.resources.disk_gib as f64, cx)
        });
        self.daemon_json.update(cx, |input, cx| {
            input.set_text(
                &self
                    .state
                    .host
                    .daemon_document()
                    .unwrap_or_else(|_| "{}".into()),
                cx,
            )
        });
        self.share_path
            .update(cx, |input, cx| input.set_text("", cx));
        self.share_writable = false;
        self.state.settings.set(cx, settings);
        self.notice = None;
        self.vm_notice = None;
        self.resource_notice = None;
        cx.notify();
    }

    fn toggle(&mut self, field: ToggleField, cx: &mut Context<Self>) {
        let mut settings = self.state.host.settings();
        match field {
            ToggleField::Autostart => settings.autostart_engine = !settings.autostart_engine,
            ToggleField::KeepAlive => settings.keep_engine_on_quit = !settings.keep_engine_on_quit,
            ToggleField::Idle => {
                settings.idle_suspend_minutes = if settings.idle_suspend_minutes.is_some() {
                    None
                } else {
                    Some(10)
                }
            }
        }
        if let Err(error) = self.state.host.save_settings(settings.clone()) {
            self.notice = Some(format!("Could not save settings: {error}"));
        } else {
            self.state.settings.set(cx, self.state.host.settings());
            self.notice = None;
        }
        cx.notify();
    }

    fn save_resources(&mut self, cx: &mut Context<Self>) {
        let mut settings = self.state.host.settings();
        let (Some(cpus), Some(memory), Some(disk)) = (
            self.cpus.read(cx).value_f64(),
            self.memory.read(cx).value_f64(),
            self.disk.read(cx).value_f64(),
        ) else {
            self.resource_notice = Some(Err(
                "Enter a number in each VM resource field before saving.".into(),
            ));
            cx.notify();
            return;
        };
        let capacity = self.state.host.engine_capacity();
        if [cpus, memory, disk]
            .iter()
            .any(|value| !value.is_finite() || value.fract() != 0.0)
            || !(1.0..=capacity.max_cpus as f64).contains(&cpus)
            || !(1.0..=capacity.max_memory_gib as f64).contains(&memory)
            || !(10.0..=2048.0).contains(&disk)
        {
            self.resource_notice = Some(Err(
                format!("Use whole numbers: 1–{} CPU cores, 1–{} GiB memory, and 10–2048 GiB disk. Hopper reserves resources for the host.", capacity.max_cpus, capacity.max_memory_gib),
            ));
            cx.notify();
            return;
        }
        settings.resources = model::EngineResources {
            cpus: cpus as u32,
            memory_gib: memory as u32,
            disk_gib: disk as u32,
        }
        .bounded();
        match self.state.host.save_settings(settings.clone()) {
            Ok(()) => {
                self.state.settings.set(cx, self.state.host.settings());
                self.resource_notice = Some(Ok(format!("Saved: {cpus} cores, {memory} GiB memory, {disk} GiB initial disk. Stop and start the engine to apply CPU and memory changes.")));
            }
            Err(error) => {
                self.resource_notice = Some(Err(format!("Could not save VM resources: {error}")))
            }
        }
        cx.notify();
    }

    fn save_vm_settings(&mut self, settings: model::Settings, cx: &mut Context<Self>) -> bool {
        match self.state.host.save_settings(settings) {
            Ok(()) => {
                self.state.settings.set(cx, self.state.host.settings());
                self.vm_notice = Some(
                    "Saved. Stop and start Hopper Engine to apply file shares and Docker settings."
                        .into(),
                );
                cx.notify();
                true
            }
            Err(error) => {
                self.vm_notice = Some(error.to_string());
                cx.notify();
                false
            }
        }
    }

    fn add_share(&mut self, cx: &mut Context<Self>) {
        let path = self.share_path.read(cx).text().trim().to_string();
        let mut settings = self.state.host.settings();
        settings.shares.push(model::SharedPath {
            path,
            writable: self.share_writable,
        });
        if self.save_vm_settings(settings, cx) {
            self.share_path
                .update(cx, |input, cx| input.set_text("", cx));
            self.share_writable = false;
        }
    }

    fn remove_share(&mut self, path: &str, cx: &mut Context<Self>) {
        let mut settings = self.state.host.settings();
        settings.shares.retain(|share| share.path != path);
        settings.shared_paths.retain(|share| share != path);
        self.save_vm_settings(settings, cx);
    }

    fn save_daemon(&mut self, cx: &mut Context<Self>) {
        let mut settings = self.state.host.settings();
        settings.daemon = model::DaemonConfig {
            extra_json: Some(self.daemon_json.read(cx).text()),
            ..Default::default()
        };
        self.save_vm_settings(settings, cx);
    }

    fn refresh_vm_stats(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        self.busy = true;
        bridge::run(
            cx,
            async move { host.engine_stats().await },
            move |stats, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.busy = false;
                    this.vm_stats = stats;
                    this.vm_notice = stats.is_none().then(|| {
                        "VM usage could not be sampled. Start Hopper Engine and retry.".into()
                    });
                    cx.notify();
                });
            },
        );
    }

    fn reclaim_vm(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        self.busy = true;
        bridge::run(
            cx,
            async move { host.reclaim_engine().await },
            move |result, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.busy = false;
                    this.vm_notice = Some(result.detail);
                    cx.notify();
                });
            },
        );
    }

    #[cfg(target_os = "macos")]
    fn configure_docker_cli(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        self.busy = true;
        self.cli_notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host.configure_docker_cli().await },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.busy = false;
                        this.cli_notice = Some(if result.ok {
                            Ok(result.detail)
                        } else {
                            Err(result.detail)
                        });
                        cx.notify();
                    });
                }
            },
        );
    }

    fn set_theme(&mut self, mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        let mut settings = self.state.host.settings();
        settings.theme = mode;
        let saved = match self.state.host.save_settings(settings.clone()) {
            Ok(()) => {
                self.state.settings.set(cx, self.state.host.settings());
                self.notice = None;
                true
            }
            Err(error) => {
                self.notice = Some(format!("Could not save settings: {error}"));
                false
            }
        };
        if saved {
            guise::theme::Theme::set_global(
                cx,
                theme::build(theme::scheme(mode, window.appearance())),
            );
        }
        cx.notify();
    }

    fn theme_button(
        &self,
        mode: ThemeMode,
        current: ThemeMode,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = match mode {
            ThemeMode::System => "theme-system",
            ThemeMode::Light => "theme-light",
            ThemeMode::Dark => "theme-dark",
        };
        Button::new(id, mode.as_str())
            .size(Size::Xs)
            .variant(if mode == current {
                Variant::Light
            } else {
                Variant::Subtle
            })
            .color(if mode == current {
                ColorName::Blue
            } else {
                ColorName::Gray
            })
            .on_click(cx.listener(move |this, _, window, cx| this.set_theme(mode, window, cx)))
    }

    fn load_choices(&mut self, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let this = cx.entity().downgrade();
        self.choices_request = self.choices_request.wrapping_add(1);
        let request = self.choices_request;
        self.loading_choices = true;
        cx.notify();
        bridge::run(
            cx,
            async move { host.engine_choices().await },
            move |choices, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if this.choices_request != request {
                            return;
                        }
                        this.choices = choices;
                        this.loading_choices = false;
                        cx.notify();
                    });
                }
            },
        );
    }

    /// Pin an engine, or hand selection back to Hopper.
    ///
    /// The whole app follows: the client is repointed, the backend swaps, and
    /// the epoch bump makes every list refetch from whichever engine answered.
    fn choose(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        let previous = self.preference.clone();
        self.preference = id.clone();
        self.busy = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host.set_engine_preference(id).await },
            move |result, cx| {
                let status = result.as_ref().ok().cloned();
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.busy = false;
                        match result {
                            Ok(_) => this.load_choices(cx),
                            Err(error) => {
                                this.preference = previous;
                                this.notice =
                                    Some(format!("Could not save engine choice: {error}"));
                            }
                        }
                        cx.notify();
                    });
                }
                if let Some(status) = status {
                    state.engine.set(cx, status);
                    state.settings.set(cx, state.host.settings());
                    state.bump(cx);
                }
            },
        );
    }

    /// Fetch Apple's signed installer and hand it to macOS.
    ///
    /// Reachable while Docker is connected and working — that is the point.
    /// Someone happy on Docker Desktop still needs a way to be offered the
    /// native runtime, and the first-run panel only shows when nothing is up.
    #[cfg(target_os = "macos")]
    fn install_apple(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let state = self.state.clone();
        self.installing = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host::appleinstall::download_and_open().await },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        this.installing = false;
                        this.notice = result.err();
                        // Opening the installer does not mean it has finished;
                        // refresh once now, then the helper copy below tells
                        // the user how to make the completed install appear.
                        this.load_choices(cx);
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }

    /// One engine in the picker.
    ///
    /// Every listed engine is pickable, including one that is not installed —
    /// pinning the engine you are moving *to* is the whole point, and the
    /// status then says what is left to do about it.
    fn engine_button(
        &self,
        id: Option<&str>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.preference.as_deref() == id;
        let pin = id.map(str::to_string);
        Button::new(
            SharedString::from(format!("engine-choice-{}", id.unwrap_or("auto"))),
            label.to_string(),
        )
        .size(Size::Xs)
        .variant(if active {
            Variant::Light
        } else {
            Variant::Subtle
        })
        .color(if active {
            ColorName::Blue
        } else {
            ColorName::Gray
        })
        .disabled(self.busy)
        .on_click(cx.listener(move |this, _, _, cx| this.choose(pin.clone(), cx)))
    }

    fn start_engine(&mut self, cx: &mut Context<Self>) {
        self.lifecycle_generation += 1;
        let generation = self.lifecycle_generation;
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.busy = true;
        self.notice = None;
        cx.notify();
        bridge::run(
            cx,
            async move { host.start_engine().await.map_err(|e| e.to_string()) },
            move |result, cx| {
                // A start that fails has to say so here. The button is in this
                // pane, so a `tracing::warn` reaches nobody who pressed it.
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if this.lifecycle_generation != generation {
                            return;
                        }
                        this.busy = false;
                        this.notice = result.err();
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }

    fn stop_engine(&mut self, cx: &mut Context<Self>) {
        self.lifecycle_generation += 1;
        let generation = self.lifecycle_generation;
        let host = Arc::clone(&self.state.host);
        let state = self.state.clone();
        let this = cx.entity().downgrade();
        self.notice = None;
        self.busy = true;
        cx.notify();
        bridge::run(
            cx,
            async move { host.stop_engine().await.map_err(|e| e.to_string()) },
            move |result, cx| {
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, cx| {
                        if this.lifecycle_generation != generation {
                            return;
                        }
                        // Keep the control usable after a failed stop and make
                        // recovery visible beside the action.
                        this.notice = result.err();
                        this.busy = false;
                        cx.notify();
                    });
                }
                state.bump(cx);
            },
        );
    }

    fn section(&self, title: &str, body: impl IntoElement, cx: &gpui::App) -> impl IntoElement {
        let palette = theme::palette(cx);
        div()
            .w_full()
            .min_w_0()
            .p_4()
            .rounded_md()
            .bg(palette.bg_subtle)
            .border_1()
            .border_color(palette.border_subtle)
            .child(
                Stack::new()
                    .gap(Size::Sm)
                    .child(Text::new(title.to_string()).size(Size::Sm).bold())
                    .child(body),
            )
    }
}

impl Render for Settings {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = theme::palette(cx);
        let engine = self.state.engine.get(cx);
        let settings = self.state.settings.get(cx);
        if !self.busy {
            self.preference = settings.engine_preference.clone();
        }

        // `NotInstalled` is deliberately absent: on macOS that means Apple's
        // runtime is not on the machine, and there is nothing to start until
        // it is. The install button below is what that state deserves.
        let can_start = matches!(
            engine.state,
            EngineState::Stopped | EngineState::Unreachable
        ) && engine.managed;
        let can_stop =
            (engine.connected || engine.state == EngineState::Starting) && engine.managed;

        let mut engine_body = Stack::new()
            .gap(Size::Sm)
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .align(Align::Center)
                    .child(
                        Badge::new(engine_state_label(engine.state))
                            .variant(Variant::Light)
                            .color(engine_state_color(engine.state))
                            .size(Size::Xs),
                    )
                    .child(
                        Text::new(provider_label(&engine.provider))
                            .size(Size::Xs)
                            .dimmed(),
                    ),
            )
            .child(Text::new(engine.message.clone()).size(Size::Xs))
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .child(
                        Button::new(
                            "engine-start",
                            if self.busy {
                                "Working…"
                            } else if engine.state == EngineState::Unreachable {
                                "Retry startup"
                            } else {
                                "Start engine"
                            },
                        )
                        .size(Size::Xs)
                        .variant(Variant::Light)
                        .color(ColorName::Green)
                        // Only a managed engine can be started from here;
                        // an engine someone else runs is theirs to control.
                        .disabled(self.busy || !can_start)
                        .on_click(cx.listener(|this, _, _, cx| this.start_engine(cx))),
                    )
                    .child(
                        Button::new("engine-stop", "Stop engine")
                            .size(Size::Xs)
                            .variant(Variant::Subtle)
                            .color(ColorName::Red)
                            .disabled(
                                !can_stop || (self.busy && engine.state != EngineState::Starting),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.stop_engine(cx))),
                    ),
            );

        if let Some(detail) = &engine.detail {
            engine_body = engine_body.child(Text::new(detail.clone()).size(Size::Xs).dimmed());
        }
        if let Some(notice) = &self.notice {
            let red = guise::theme::theme(cx).color(ColorName::Red, 6);
            engine_body = engine_body.child(Text::new(notice.clone()).size(Size::Xs).color(red));
        }

        // Which engine answers. Automatic prefers an engine that is already
        // connected, then uses the platform's managed runtime when it needs
        // to provide one.
        let mut buttons = Group::new()
            .gap(Size::Xs)
            .wrap(true)
            .child(self.engine_button(None, "Automatic", cx))
            .child(
                Button::new(
                    "engine-refresh",
                    if self.loading_choices {
                        "Refreshing…"
                    } else {
                        "Refresh"
                    },
                )
                .size(Size::Xs)
                .variant(Variant::Subtle)
                .color(ColorName::Gray)
                .disabled(self.busy || self.loading_choices)
                .on_click(cx.listener(|this, _, _, cx| this.load_choices(cx))),
            );
        for choice in &self.choices {
            buttons = buttons.child(self.engine_button(Some(&choice.id), &choice.label, cx));
        }

        let mut choice_body = Stack::new().gap(Size::Sm).child(buttons).child(
            Text::new(match self.preference.as_deref() {
                None => {
                    "Hopper uses a connected engine when available, otherwise its own Linux VM."
                }
                Some(_) => "Pinned. Hopper will not move off this engine on its own.",
            })
            .size(Size::Xs)
            .dimmed(),
        );

        // One row per engine: where it listens, or why it cannot be picked.
        // A machine with Docker Desktop and Podman both installed shows two
        // buttons that would otherwise say nothing but "Connected." — the
        // socket is what tells them apart. And an engine that is missing says
        // so, rather than leaving a dead button with no explanation.
        let mut rows = Stack::new().gap(Size::Xs);
        for choice in &self.choices {
            let (status_label, status_color) = choice_status(choice);
            let detail = match (&choice.endpoint, &choice.reason) {
                (Some(endpoint), _) if choice.connected => endpoint.clone(),
                // Apple's runtime is the one engine with nowhere to point.
                (None, _) if choice.connected => {
                    "No Docker socket; Hopper drives this engine directly.".to_string()
                }
                (Some(endpoint), Some(reason)) => format!("{reason} ({endpoint})"),
                (_, Some(reason)) => reason.clone(),
                (Some(endpoint), _) => endpoint.clone(),
                _ => continue,
            };
            rows = rows.child(
                Group::new()
                    .gap(Size::Sm)
                    .align(Align::Center)
                    .child(
                        div()
                            .min_w(px(150.0))
                            .child(Text::new(choice.label.clone()).size(Size::Xs)),
                    )
                    .child(
                        Badge::new(status_label)
                            .variant(Variant::Light)
                            .color(status_color)
                            .size(Size::Xs),
                    )
                    .child(Text::new(detail).size(Size::Xs).dimmed()),
            );
        }
        choice_body = choice_body.child(rows);

        // The way off Docker Desktop, offered while Docker is still working.
        // Gated on the Mac being able to run it: an install cannot fix macOS 25.
        #[cfg(target_os = "macos")]
        if cfg!(target_arch = "aarch64")
            && self.choices.iter().any(|c| c.id == "apple" && !c.available)
            && apple::system::too_old().is_none()
        {
            choice_body = choice_body.child(
                Group::new().gap(Size::Xs).child(
                    Button::new(
                        "engine-install-apple",
                        if self.installing {
                            "Downloading…"
                        } else {
                            "Install Apple Containers"
                        },
                    )
                    .size(Size::Xs)
                    .variant(Variant::Filled)
                    .color(ColorName::Blue)
                    .disabled(self.installing)
                    .on_click(cx.listener(|this, _, _, cx| this.install_apple(cx))),
                ),
            );
            choice_body = choice_body.child(
                Text::new(
                    "Complete the macOS installer, then press Refresh to detect Apple Containers.",
                )
                .size(Size::Xs)
                .dimmed(),
            );
        }

        // Apple's runtime answers no socket, so there is nothing for
        // `DOCKER_HOST` to name — printing the engine's label there would be a
        // line that looks like a command and works like nothing.
        let cli_body = if self.state.host.runtime_kind() == RuntimeKind::Apple {
            Stack::new()
                .gap(Size::Xs)
                .child(Text::new("container ls").size(Size::Xs))
                .child(
                    Text::new(
                        "Apple Containers publishes no Docker socket, so `docker` cannot be \
                         pointed at it. Apple's own `container` command drives the same \
                         runtime Hopper is showing.",
                    )
                    .size(Size::Xs)
                    .dimmed(),
                )
        } else {
            let docker_host = self.state.host.client().endpoint().docker_host_value();
            let mut body = Stack::new()
                .gap(Size::Xs)
                .child(Text::new(format!("export DOCKER_HOST=\"{}\"", docker_host)).size(Size::Xs))
                .child(
                    Text::new(
                        "Install Hopper's bundled Docker, Compose, and Buildx tools and activate \
                         a Docker context that targets this engine.",
                    )
                    .size(Size::Xs)
                    .dimmed(),
                );
            #[cfg(target_os = "macos")]
            {
                body = body
                    .child(
                        Button::new("configure-docker-cli", "Install Docker CLI tools")
                            .size(Size::Xs)
                            .variant(Variant::Light)
                            .color(ColorName::Blue)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.configure_docker_cli(cx))),
                    )
                    .children(self.cli_notice.as_ref().map(|notice| {
                        match notice {
                            Ok(message) => Text::new(message.clone()).size(Size::Xs).dimmed(),
                            Err(message) => Text::new(message.clone())
                                .size(Size::Xs)
                                .color(guise::theme::theme(cx).color(ColorName::Red, 6)),
                        }
                    }));
            }
            body
        };

        let theme_body = Stack::new()
            .gap(Size::Xs)
            .child(
                Group::new()
                    .gap(Size::Xs)
                    .child(self.theme_button(ThemeMode::System, settings.theme, cx))
                    .child(self.theme_button(ThemeMode::Light, settings.theme, cx))
                    .child(self.theme_button(ThemeMode::Dark, settings.theme, cx)),
            )
            .child(
                Text::new("System follows the operating system appearance.")
                    .size(Size::Xs)
                    .dimmed(),
            );
        let lifecycle_body = Stack::new()
            .gap(Size::Sm)
            .child(
                Group::new().gap(Size::Sm).align(Align::Center)
                    .child(Switch::new("autostart-engine").size(Size::Sm).checked(settings.autostart_engine).on_change(cx.listener(|this, _, _, cx| this.toggle(ToggleField::Autostart, cx))))
                    .child(Stack::new().gap(Size::Xs).child(Text::new("Start the selected engine on launch").size(Size::Xs)).child(Text::new("Only engines Hopper manages are started.").size(Size::Xs).dimmed())),
            )
            .child(
                Group::new().gap(Size::Sm).align(Align::Center)
                    .child(Switch::new("keep-engine-alive").size(Size::Sm).checked(settings.keep_engine_on_quit).on_change(cx.listener(|this, _, _, cx| this.toggle(ToggleField::KeepAlive, cx))))
                    .child(Stack::new().gap(Size::Xs).child(Text::new("Keep the engine running after closing Hopper").size(Size::Xs)).child(Text::new("Useful for CLI and CI workloads; uses resources until stopped.").size(Size::Xs).dimmed())),
            );
        let lifecycle_body = lifecycle_body.child(
            Group::new().gap(Size::Sm).align(Align::Center)
                .child(Switch::new("idle-engine").size(Size::Sm).checked(settings.idle_suspend_minutes.is_some()).on_change(cx.listener(|this, _, _, cx| this.toggle(ToggleField::Idle, cx))))
                .child(Stack::new().gap(Size::Xs)
                    .child(Text::new(format!("Stop an empty Hopper Engine after {} minutes", settings.idle_suspend_minutes.unwrap_or(10))).size(Size::Xs))
                    .child(Text::new("Opt-in. Any existing container prevents stopping. CLI clients need you to start the engine again; no automatic wake.").size(Size::Xs).dimmed()))
        );
        let resource_body = if engine.provider == "vm" {
            Stack::new().gap(Size::Sm)
                .child(Text::new("One Linux VM runs your containers. CPU and memory changes apply after stopping and starting the engine.").size(Size::Xs))
                .child(Group::new().gap(Size::Sm).align(Align::Start).wrap(true)
                    .child(div().w(px(180.0)).h(px(84.0)).child(self.cpus.clone()))
                    .child(div().w(px(180.0)).h(px(84.0)).child(self.memory.clone()))
                    .child(div().w(px(180.0)).h(px(84.0)).child(self.disk.clone())))
                .child(Text::new("Disk size applies when the VM is first created. Existing disks and volumes are kept. Published ports are available on localhost. CPU and memory limits reserve capacity for the host.").size(Size::Xs).dimmed())
                .child(Button::new("engine-save-resources", "Save VM resources")
                    .size(Size::Xs).variant(Variant::Light).color(ColorName::Blue)
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| this.save_resources(cx))))
                .children(self.resource_notice.as_ref().map(|notice| match notice {
                    Ok(message) => Text::new(message.clone()).size(Size::Xs).dimmed(),
                    Err(message) => Text::new(message.clone()).size(Size::Xs).color(guise::theme::theme(cx).color(ColorName::Red, 6)),
                }))
                .into_any_element()
        } else if self.state.host.runtime_kind() == RuntimeKind::Apple {
            Stack::new()
                .gap(Size::Xs)
                .child(
                    Text::new("Apple sizes each container's lightweight VM when it starts.")
                        .size(Size::Xs),
                )
                .child(
                    Text::new(
                        "Set CPU and memory limits in the Run dialog or Compose file; there is no global VM budget for this backend.",
                    )
                    .size(Size::Xs)
                    .dimmed(),
                )
                .into_any_element()
        } else {
            Stack::new()
                .gap(Size::Xs)
                .child(
                    Text::new(
                        "Docker and Podman manage their own daemon or VM resources. Set container limits in the Run dialog or Compose file.",
                    )
                    .size(Size::Xs),
                )
                .child(
                    Text::new(
                        "Hopper does not start or resize an Engine API daemon, so it will not consume a hidden VM budget.",
                    )
                    .size(Size::Xs)
                    .dimmed(),
                )
                .into_any_element()
        };

        let vm_settings = if engine.provider == "vm" {
            let mut shares = Stack::new().gap(Size::Sm)
                .child(Text::new("Shares for the next engine start").size(Size::Sm).medium())
                .child(Text::new("Only listed directories will be shared. Grants are read-only by default. Current shares remain in effect until you stop and start the engine.").size(Size::Xs).dimmed());
            for (index, share) in settings
                .shares
                .iter()
                .cloned()
                .chain(settings.shared_paths.iter().map(|path| model::SharedPath {
                    path: path.clone(),
                    writable: false,
                }))
                .enumerate()
            {
                let path = share.path.clone();
                shares = shares.child(
                    Group::new()
                        .gap(Size::Sm)
                        .wrap(true)
                        .child(Text::new(share.path).size(Size::Xs))
                        .child(
                            Badge::new(if share.writable {
                                "Read/write"
                            } else {
                                "Read-only"
                            })
                            .size(Size::Xs),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("remove-share-{index}")),
                                "Remove",
                            )
                            .size(Size::Xs)
                            .variant(Variant::Subtle)
                            .disabled(self.busy)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.remove_share(&path, cx)),
                            ),
                        ),
                );
            }
            if settings.shares.is_empty() && settings.shared_paths.is_empty() {
                shares = shares.child(
                    Text::new("No host directories are shared on the next start.")
                        .size(Size::Xs)
                        .dimmed(),
                );
            }
            shares = shares.child(div().w_full().min_w_0().flex().flex_col().h(px(84.0)).child(self.share_path.clone()))
                .child(Group::new().gap(Size::Sm).wrap(true)
                    .child(Switch::new("share-writable").checked(self.share_writable).size(Size::Sm).on_change(cx.listener(|this, _, _, cx| {this.share_writable = !this.share_writable; cx.notify();})))
                    .child(Text::new("Allow writes to this directory").size(Size::Xs))
                    .child(Button::new("add-share", "Add directory").size(Size::Xs).variant(Variant::Light).disabled(self.busy).on_click(cx.listener(|this, _, _, cx| this.add_share(cx)))))
                .child(Text::new("Docker defaults limit new container logs and build-cache retention. Advanced JSON is validated before saving, then validated by Docker before activation; a failed activation restores the previous configuration.").size(Size::Xs).dimmed())
                .child(div().w_full().min_w_0().flex().flex_col().h(px(240.0)).child(self.daemon_json.clone()))
                .child(Button::new("save-daemon", "Save Docker settings").size(Size::Xs).variant(Variant::Light).disabled(self.busy).on_click(cx.listener(|this, _, _, cx| this.save_daemon(cx))))
                .child(Group::new().gap(Size::Sm).wrap(true)
                    .child(Button::new("vm-stats", "Refresh VM usage").size(Size::Xs).variant(Variant::Light).disabled(self.busy || !engine.connected).on_click(cx.listener(|this, _, _, cx| this.refresh_vm_stats(cx))))
                    .child(Button::new("vm-reclaim", "Reclaim unused disk blocks").size(Size::Xs).variant(Variant::Light).disabled(self.busy || !engine.connected).on_click(cx.listener(|this, _, _, cx| this.reclaim_vm(cx)))));
            if let Some(stats) = self.vm_stats {
                shares = shares.child(Text::new(format!("VM memory: {} MiB used / {} MiB total · disk: {} MiB used / {} MiB total · load: {:.2}", (stats.mem_total_kb - stats.mem_avail_kb) / 1024, stats.mem_total_kb / 1024, stats.disk_used_kb / 1024, stats.disk_total_kb / 1024, stats.load1)).size(Size::Xs));
            }
            if let Some(notice) = &self.vm_notice {
                shares = shares.child(Text::new(notice.clone()).size(Size::Xs));
            }
            Some(
                self.section("VM files, Docker, and usage", shares, cx)
                    .into_any_element(),
            )
        } else {
            None
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .p_4()
                    .border_b_1()
                    .border_color(palette.border)
                    .justify_between()
                    .child(Text::new("Settings").size(Size::Xl).bold())
                    .child(
                        Button::new("reload-settings", "Reload saved values")
                            .size(Size::Xs)
                            .variant(Variant::Subtle)
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.reload_saved(cx))),
                    ),
            )
            .child(
                ScrollArea::new("settings-scroll").fill().child(
                    div().p_4().child(
                        Stack::new()
                            .gap(Size::Md)
                            .child(self.section("Engine", engine_body, cx))
                            .child(self.section("Which engine", choice_body, cx))
                            .child(self.section("Coexist or replace", self.replacement.clone(), cx))
                            .child(self.section("Docker CLI", cli_body, cx))
                            .child(self.section("Lifecycle", lifecycle_body, cx))
                            .child(self.section(
                                if engine.provider == "vm" {
                                    "VM resources"
                                } else {
                                    "Container resources"
                                },
                                resource_body,
                                cx,
                            ))
                            .children(vm_settings)
                            .child(self.section("Appearance", theme_body, cx)),
                    ),
                ),
            )
    }
}

#[derive(Clone, Copy)]
enum ToggleField {
    Autostart,
    KeepAlive,
    Idle,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, available: bool, connected: bool) -> EngineChoice {
        EngineChoice {
            id: id.into(),
            label: id.into(),
            available,
            installed: false,
            connected,
            state: if connected {
                EngineState::Connected
            } else {
                EngineState::NotInstalled
            },
            managed: false,
            reason: None,
            endpoint: None,
        }
    }

    #[test]
    fn engine_states_are_written_for_people_not_serialization() {
        assert_eq!(
            engine_state_label(EngineState::NotInstalled),
            "Not installed"
        );
        assert_eq!(
            engine_state_label(EngineState::NeedsPermission),
            "Needs permission"
        );
        assert_eq!(
            engine_state_label(EngineState::Unreachable),
            "Not responding"
        );
    }

    #[test]
    fn unhealthy_engine_states_are_visually_distinct() {
        assert_eq!(engine_state_color(EngineState::Connected), ColorName::Green);
        assert_eq!(
            engine_state_color(EngineState::NeedsPermission),
            ColorName::Orange
        );
        assert_eq!(engine_state_color(EngineState::Starting), ColorName::Blue);
    }

    #[test]
    fn existing_fallback_is_not_called_available_when_nothing_answers() {
        assert_eq!(
            choice_status(&choice("existing", true, false)).0,
            "Not installed"
        );
        assert_eq!(choice_status(&choice("docker", true, false)).0, "Available");
        assert_eq!(
            choice_status(&choice("existing", true, true)).0,
            "Connected"
        );
    }

    #[test]
    fn an_installed_stopped_app_is_not_labeled_missing() {
        let mut stopped = choice("docker", false, false);
        stopped.installed = true;
        assert_eq!(choice_status(&stopped).0, "Installed");
    }

    #[test]
    fn a_stale_socket_is_shown_as_not_responding() {
        let mut stale = choice("docker", true, false);
        stale.state = EngineState::Unreachable;
        assert_eq!(choice_status(&stale).0, "Not responding");
        assert_eq!(choice_status(&stale).1, ColorName::Orange);
    }
}
