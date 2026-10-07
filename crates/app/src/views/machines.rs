mod viewer;
mod native;
mod start;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod preparation;
use gpui::prelude::*;
use gpui::{div, px, Context, Entity, PathPromptOptions, SharedString, Window};
use guise::prelude::*;
use host::MachineActor;
use model::{CreateMachine, EngineResources, MachineStatus};
use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use crate::bridge;
use crate::state::{AppState, Load};
use crate::theme;

pub struct Machines {
    state: AppState,
    rows: Load<Vec<MachineStatus>>,
    busy: BTreeMap<String, String>,
    error: Option<String>,
    creating: bool,
    profile: String,
    name: Entity<TextInput>,
    cpus: Entity<NumberInput>,
    memory: Entity<NumberInput>,
    disk: Entity<NumberInput>,
    installer: Option<String>,
    agent_access: bool,
    last_epoch: u64,
    snapshots_for: Option<String>,
    snapshots: Vec<model::MachineSnapshot>,
    restore: Option<(String, String)>,
}

impl Machines {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::get(cx);
        watch(cx, &state.epoch);
        let mut view = Self {
            state,
            rows: Load::Loading,
            busy: BTreeMap::new(),
            error: None,
            creating: false,
            profile: "ubuntu".into(),
            name: cx.new(|cx| {
                TextInput::new(cx)
                    .label("Name")
                    .placeholder("My development VM")
            }),
            cpus: cx.new(|cx| {
                NumberInput::new(cx)
                    .min(1.0)
                    .max(64.0)
                    .step(1.0)
                    .value(4.0)
                    .label("CPUs")
            }),
            memory: cx.new(|cx| {
                NumberInput::new(cx)
                    .min(1.0)
                    .max(256.0)
                    .step(1.0)
                    .value(4.0)
                    .label("Memory (GiB)")
            }),
            disk: cx.new(|cx| {
                NumberInput::new(cx)
                    .min(10.0)
                    .max(2048.0)
                    .step(1.0)
                    .value(64.0)
                    .label("Disk (GiB)")
            }),
            installer: None,
            agent_access: true,
            last_epoch: 0,
            snapshots_for: None,
            snapshots: vec![],
            restore: None,
        };
        view.reload(cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            if this.update(cx, |this, cx| this.reload(cx)).is_err() {
                break;
            }
        })
        .detach();
        view
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let host = self.state.host.clone();
        let weak = cx.entity().downgrade();
        bridge::run(
            cx,
            async move { host.list_machines(MachineActor::Person).await },
            move |result, cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        this.rows = match result {
                            Ok(rows) => Load::Ready(rows),
                            Err(error) => Load::Failed(format!("{error:#}")),
                        };
                        cx.notify();
                    });
                }
            },
        );
    }

    fn operate(
        &mut self,
        id: String,
        message: &str,
        future: impl Future<Output = anyhow::Result<()>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy.insert(id.clone(), message.into());
        self.error = None;
        cx.notify();
        let weak = cx.entity().downgrade();
        bridge::run(cx, future, move |result, cx| {
            if let Some(view) = weak.upgrade() {
                view.update(cx, |this, cx| {
                    this.busy.remove(&id);
                    if let Err(error) = result {
                        this.error = Some(format!("{error:#}"));
                    } else if id == "create" {
                        this.creating = false;
                    }
                    this.reload(cx);
                    cx.notify();
                });
            }
        });
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let values = [
            self.cpus.read(cx).value_f64().unwrap_or(f64::NAN),
            self.memory.read(cx).value_f64().unwrap_or(f64::NAN),
            self.disk.read(cx).value_f64().unwrap_or(f64::NAN),
        ];
        if values.iter().any(|v| !v.is_finite() || v.fract() != 0.0) {
            self.error = Some("VM resources must be whole numbers".into());
            cx.notify();
            return;
        }
        let request = CreateMachine {
            name: self.name.read(cx).text().trim().into(),
            profile: self.profile.clone(),
            resources: EngineResources {
                cpus: values[0] as u32,
                memory_gib: values[1] as u32,
                disk_gib: values[2] as u32,
            },
            installer: self.installer.clone(),
            agent_access: self.agent_access,
        };
        let host = self.state.host.clone();
        self.operate(
            "create".into(),
            "Creating…",
            async move {
                host.create_machine(request).await?;
                Ok(())
            },
            cx,
        );
    }

    fn installer(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(if self.profile == "ubuntu" { "Use Linux ARM64 ISO" } else { "Use Windows ARM64 ISO" }.into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                if let Some(path) = paths.first() {
                    let path = path.to_string_lossy().into_owned();
                    let _ = this.update(cx, |this, cx| {
                        this.installer = Some(path);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    fn show_snapshots(&mut self, id: String, cx: &mut Context<Self>) {
        match self
            .state
            .host
            .machines()
            .snapshots(&id, MachineActor::Person)
        {
            Ok(snapshots) => {
                self.snapshots = snapshots;
                self.snapshots_for = Some(id);
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
        cx.notify();
    }

    fn row(&self, row: &MachineStatus, cx: &mut Context<Self>) -> impl IntoElement {
        let machine = &row.machine;
        let id = machine.id.clone();
        let manager = self.state.host.machines();
        let running = row.state == "Running" || row.state == "Paused";
        let windows = machine.guest == model::GuestOs::Windows;
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let virtual_owned = crate::machines::owns(&id, cx);
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        let virtual_owned = false;
        let row_native = machine.runtime == Some(model::MachineRuntime::Virtualization);
        let start_name = machine.name.clone();
        let stopped = row.state == "Stopped";
        let uncreated = row.state == "Not created";
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        let automatic = crate::machines::pending(&id, cx);
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        let automatic = false;
        let busy = self.busy.contains_key(&id) || row.busy || automatic;
        let mut progress = row.progress.clone();
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        if let Some(message) = crate::machines::message(&id, cx) { progress = Some(message); }
        if automatic { progress = Some("Starting Ubuntu automatically…".into()); }
        let toggle_id = id.clone();
        let start_id = id.clone();
        let clone_id = id.clone();
        let snapshot_id = id.clone();
        let history_id = id.clone();
        let clone_name = format!("{} copy", machine.name);
        let phase = self.busy.get(&id).cloned().unwrap_or_else(|| {
            if row.busy {
                "Working…".into()
            } else {
                row.state.clone()
            }
        });
        let access = machine.agent_access;
        let palette = theme::palette(cx);
        let view_id = id.clone();
        let view_name = machine.name.clone();
        let mut controls = Group::new()
            .gap(Size::Xs)
            .wrap(true)
            .child(
                Button::new(
                    SharedString::from(format!("start-{id}")),
                    if running { "Stop" } else { "Start & view" },
                )
                .size(Size::Sm)
                .variant(Variant::Light)
                .color(ColorName::Blue)
                .disabled(busy || (!running && !stopped && !uncreated && row.state != "Ready to start"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let manager = manager.clone();
                    let id = start_id.clone();
                    if windows {
                        this.start_windows(id, start_name.clone(), running, cx);
                        return;
                    }
                    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                    if virtual_owned || row_native {
                        this.start_virtual(id, start_name.clone(), running, virtual_owned, cx);
                        return;
                    }
                    let operation_id = id.clone();
                    this.operate(
                        operation_id,
                        if running {
                            "Stopping…"
                        } else {
                            "Downloading & starting…"
                        },
                        async move {
                            if running {
                                manager.stop(&id, MachineActor::Person).await
                            } else {
                                manager.start(&id, MachineActor::Person).await
                            }
                        },
                        cx,
                    );
                })),
            )
            .child(
                Button::new(SharedString::from(format!("snapshot-{id}")), "Snapshot")
                    .size(Size::Sm)
                    .variant(Variant::Subtle)
                    .disabled(busy || !stopped || windows || virtual_owned || row_native)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let manager = this.state.host.machines();
                        let id = snapshot_id.clone();
                        let name = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                        this.operate(
                            id.clone(),
                            "Saving snapshot…",
                            async move {
                                manager.snapshot(&id, MachineActor::Person, &name).await?;
                                Ok(())
                            },
                            cx,
                        );
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("clone-{id}")), "Clone")
                    .size(Size::Sm)
                    .variant(Variant::Subtle)
                    .disabled(busy || !stopped || windows || virtual_owned || row_native)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let manager = this.state.host.machines();
                        let id = clone_id.clone();
                        let name = clone_name.clone();
                        this.operate(
                            id.clone(),
                            "Cloning…",
                            async move {
                                manager
                                    .clone_machine(&id, MachineActor::Person, &name)
                                    .await?;
                                Ok(())
                            },
                            cx,
                        );
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("history-{id}")), "Snapshots")
                    .size(Size::Sm)
                    .variant(Variant::Subtle)
                    .disabled(busy || windows || virtual_owned || row_native)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.show_snapshots(history_id.clone(), cx)
                    })),
            );
        if running {
            controls = controls.child(
                Button::new(SharedString::from(format!("view-{id}")), "View")
                    .size(Size::Sm)
                    .variant(Variant::Light)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let manager = this.state.host.machines();
                        let host = this.state.host.clone();
                        let id = view_id.clone();
                        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                        if crate::machines::owns(&id, cx) {
                            this.error = crate::machines::open(&id, &view_name, cx).err().map(|error| format!("{error:#}"));
                            cx.notify();
                            return;
                        }
                        let identity = id.clone();
                        let name = view_name.clone();
                        let weak = cx.entity().downgrade();
                        this.error = None;
                        bridge::run(
                            cx,
                            async move {
                                if windows {
                                    Ok(None)
                                } else {
                                    manager.viewer_pid(&id).await.map(Some)
                                }
                            },
                            move |result, cx| {
                                if let Some(view) = weak.upgrade() {
                                    view.update(cx, |this, cx| {
                                        let shown = result.and_then(|pid| match pid {
                                            Some(pid) => viewer::show(pid),
                                            None => native::open(host, identity, name, cx),
                                        });
                                        if let Err(error) = shown {
                                            this.error = Some(format!("{error:#}"));
                                        }
                                        cx.notify();
                                    });
                                }
                            },
                        );
                    })),
            );
        }
        if !cfg!(target_os = "macos") {
            controls = Group::new().child(Text::new("Guest VMs require macOS"));
        }
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .border_b_1()
            .border_color(palette.border_subtle)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        Stack::new()
                            .gap(Size::Xs)
                            .child(
                                Group::new()
                                    .gap(Size::Sm)
                                    .child(Text::new(machine.name.clone()).size(Size::Sm).medium())
                                    .child(
                                        Badge::new(phase)
                                            .variant(Variant::Light)
                                            .color(if running {
                                                ColorName::Green
                                            } else {
                                                ColorName::Gray
                                            })
                                            .size(Size::Xs),
                                    ),
                            )
                            .child(
                                Text::new(format!(
                                    "{} · {} CPUs · {} GiB memory · {} GiB disk",
                                    machine.guest.label(),
                                    machine.resources.cpus,
                                    machine.resources.memory_gib,
                                    machine.resources.disk_gib
                                ))
                                .size(Size::Xs)
                                .dimmed(),
                            ),
                    )
                    .child(
                        Group::new()
                            .gap(Size::Xs)
                            .child(Text::new("Agent access").size(Size::Xs))
                            .child(
                                Switch::new(SharedString::from(format!("agent-{id}")))
                                    .size(Size::Sm)
                                    .checked(machine.agent_access)
                                    .on_change(cx.listener(move |this, _, _, cx| {
                                        if let Err(error) = this
                                            .state
                                            .host
                                            .machines()
                                            .set_agent_access(&toggle_id, !access)
                                        {
                                            this.error = Some(error.to_string());
                                        }
                                        this.reload(cx);
                                    })),
                            ),
                    ),
            )
            .when(progress.is_some(), |view| {
                view.child(
                    Text::new(progress.clone().unwrap())
                        .size(Size::Xs)
                        .dimmed(),
                )
            })
            .child(controls)
            .when(windows, |view| view.child(Text::new(
                "Guest tools, agent connections, snapshots and clones are not available yet"
            ).size(Size::Xs).dimmed()))
    }
}

impl Machines {
    fn history(&self, id: &str, name: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let id = id.to_string();
        let mut snapshots = Stack::new().gap(Size::Sm).child(
            Group::new()
                .justify(Justify::Between)
                .child(
                    Text::new(format!("Disk snapshots · {name}"))
                        .size(Size::Sm)
                        .medium(),
                )
                .child(
                    Button::new("close-snapshots", "Close")
                        .size(Size::Xs)
                        .variant(Variant::Subtle)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.snapshots_for = None;
                            cx.notify();
                        })),
                ),
        );
        if self.snapshots.is_empty() {
            snapshots = snapshots.child(
                Text::new("No snapshots yet. Stop this VM, then choose Snapshot.")
                    .size(Size::Xs)
                    .dimmed(),
            );
        }
        let stopped = self.rows.ready().is_some_and(|rows| {
            rows.iter()
                .any(|row| row.machine.id == id && row.state == "Stopped" && !row.busy)
        });
        if !stopped {
            snapshots = snapshots.child(
                Text::new("Stop this VM before restoring a snapshot.")
                    .size(Size::Xs)
                    .dimmed(),
            );
        }
        for snapshot in &self.snapshots {
            let sid = snapshot.id.clone();
            let mid = id.clone();
            snapshots = snapshots.child(
                Button::new(
                    SharedString::from(format!("restore-{sid}")),
                    format!("Restore {}", snapshot.name),
                )
                .size(Size::Sm)
                .variant(Variant::Light)
                .disabled(self.busy.contains_key(&id) || !stopped)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.restore = Some((mid.clone(), sid.clone()));
                    cx.notify();
                })),
            );
        }
        div().p_4().child(snapshots)
    }
}

impl Render for Machines {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let epoch = self.state.epoch.get(cx);
        if epoch != self.last_epoch {
            self.last_epoch = epoch;
            self.reload(cx);
        }
        let palette = theme::palette(cx);
        let profiles = self.state.host.machines().profiles();
        let create_busy = self.busy.contains_key("create");
        let mut body = div()
            .id("machines-scroll")
            .w_full()
            .min_w_0()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        if self.creating {
            let mut choices = Group::new().gap(Size::Xs).wrap(true);
            for profile in &profiles {
                let id = profile.id.clone();
                choices = choices.child(
                    Button::new(
                        SharedString::from(format!("profile-{id}")),
                        profile.name.clone(),
                    )
                    .size(Size::Sm)
                    .variant(if id == self.profile {
                        Variant::Light
                    } else {
                        Variant::Subtle
                    })
                    .color(ColorName::Blue)
                    .disabled(create_busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profile = id.clone();
                        let windows = id == "windows";
                        this.cpus.update(cx, |input, cx| {
                            input.set_max(if windows { 2.0 } else { 64.0 }, cx);
                        });
                        this.memory.update(cx, |input, cx| {
                            input.set_max(if windows { 64.0 } else { 256.0 }, cx);
                        });
                        this.disk.update(cx, |input, cx| {
                            input.set_min(if windows { 64.0 } else { 10.0 }, cx);
                        });
                        cx.notify();
                    })),
                );
            }
            let profile = profiles.iter().find(|p| p.id == self.profile).unwrap();
            let mut form = Stack::new()
                .gap(Size::Md)
                .child(choices)
                .child(
                    Text::new(profile.description.clone())
                        .size(Size::Xs)
                        .dimmed(),
                )
                .child(div().max_w(px(560.0)).child(self.name.clone()))
                .child(
                    Group::new()
                        .gap(Size::Md)
                        .wrap(true)
                        .align(Align::Start)
                        .child(div().w(px(160.0)).h(px(84.0)).child(self.cpus.clone()))
                        .child(div().w(px(160.0)).h(px(84.0)).child(self.memory.clone()))
                        .child(div().w(px(160.0)).h(px(84.0)).child(self.disk.clone())),
                )
                .child(
                    Group::new()
                        .gap(Size::Xs)
                        .child(
                            Switch::new("new-vm-agent")
                                .checked(self.agent_access)
                                .disabled(create_busy)
                                .on_change(cx.listener(|this, _, _, cx| {
                                    this.agent_access = !this.agent_access;
                                    cx.notify();
                                })),
                        )
                        .child(Text::new("Allow agents to control this VM").size(Size::Sm)),
                );
            if profile.experimental {
                form = form.child(
                    Badge::new("Experimental guest support")
                        .variant(Variant::Light)
                        .color(ColorName::Orange)
                        .size(Size::Xs),
                );
            }
            if profile.guest == model::GuestOs::Windows || profile.guest == model::GuestOs::Linux {
                let automatic = if profile.guest == model::GuestOs::Linux {
                    "Ubuntu downloads its ARM64 desktop installer automatically on first start. Installation and guest tools are still in development."
                } else {
                    "Windows downloads from Microsoft and prepares its installer automatically on first start."
                };
                form = form.child(Group::new().gap(Size::Sm).wrap(true)
                    .child(Button::new("choose-installer", "Use a local ARM64 ISO…")
                        .size(Size::Sm).variant(Variant::Subtle).disabled(create_busy)
                        .on_click(cx.listener(|this, _, _, cx| this.installer(cx))))
                    .when(self.installer.is_some(), |group| group.child(
                        Button::new("automatic-installer", "Use automatic download")
                            .size(Size::Sm).variant(Variant::Subtle).disabled(create_busy)
                            .on_click(cx.listener(|this, _, _, cx| { this.installer = None; cx.notify(); })))))
                    .child(Text::new(self.installer.clone().unwrap_or_else(||
                        automatic.into()))
                        .size(Size::Xs).dimmed());
            }
            form=form.child(Text::new("OS installation and guest tools are still in development. No host folders are shared.").size(Size::Xs).dimmed())
                .child(div().w(px(180.0)).child(Button::new("create-vm",self.busy.get("create").cloned().unwrap_or_else(||"Create VM".into()))
                    .size(Size::Sm).color(ColorName::Blue).disabled(create_busy)
                    .on_click(cx.listener(|this,_,_,cx|this.create(cx)))));
            body = body.child(
                div()
                    .p_4()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(form),
            );
        }
        if let Some(error) = &self.error {
            body = body.child(crate::views::failure("VM operation failed", error));
        }
        body = body.child(match &self.rows {
            Load::Loading => crate::views::message("Loading virtual machines…"),
            Load::Failed(error) => crate::views::failure("Could not load virtual machines", error),
            Load::Ready(rows) if rows.is_empty() => crate::views::message(
                "Create a Linux, macOS, or Windows VM. Each desktop opens in its own window.",
            ),
            Load::Ready(rows) => {
                let mut list = div();
                for row in rows {
                    list = list.child(self.row(row, cx));
                    if self.snapshots_for.as_deref() == Some(&row.machine.id) {
                        list = list.child(self.history(&row.machine.id, &row.machine.name, cx));
                    }
                }
                list.into_any_element()
            }
        });
        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .border_b_1()
                    .border_color(palette.border)
                    .child(Text::new("Virtual machines").size(Size::Xl).bold())
                    .child(
                        Button::new(
                            "new-vm",
                            if self.creating {
                                "Close form"
                            } else {
                                "New VM"
                            },
                        )
                        .size(Size::Sm)
                        .variant(Variant::Light)
                        .color(ColorName::Blue)
                        .disabled(create_busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.creating = !this.creating;
                            cx.notify();
                        })),
                    ),
            )
            .child(body);
        if let Some((id, snapshot)) = self.restore.clone() {
            let machine_name = self
                .state
                .host
                .machines()
                .machine(&id, MachineActor::Person)
                .map(|machine| machine.name)
                .unwrap_or_else(|_| id.clone());
            let snapshot_name = self
                .snapshots
                .iter()
                .find(|item| item.id == snapshot)
                .map(|item| item.name.clone())
                .unwrap_or_else(|| snapshot.clone());
            root=root.child(ConfirmModal::new().title("Restore disk snapshot?")
                .message(format!("Restore {snapshot_name} for {machine_name}. Changes since this snapshot will be replaced; the previous disk is retained for recovery."))
                .confirm_label("Restore snapshot")
                .on_confirm(cx.listener(move |this,_,_,cx| {
                    this.restore=None;let manager=this.state.host.machines();let id=id.clone();let snapshot=snapshot.clone();
                    this.operate(id.clone(),"Restoring snapshot…",async move {manager.restore(&id,MachineActor::Person,&snapshot).await},cx);
                }))
                .on_cancel(cx.listener(|this,_,_,cx|{this.restore=None;cx.notify();})));
        }
        root
    }
}
