mod viewer;

use gpui::{App, Global, Task};
use host::{Host, VirtualMachineOwner};
use std::{collections::BTreeMap, time::Duration};

struct Runtime {
  owner: VirtualMachineOwner,
  _poll: Task<()>,
  viewers: BTreeMap<String, viewer::Viewer>,
}
impl Global for Runtime {}

pub fn install(host: &Host, cx: &mut App) -> anyhow::Result<()> {
  anyhow::ensure!(
    !cx.has_global::<Runtime>(),
    "VZ ownership is already installed"
  );
  let owner = host.virtual_machine_owner()?;
  let wake = owner.wake();
  let task = cx.spawn(async move |cx| loop {
    let Ok(active) = cx.update(|cx| {
      let owner = &mut cx.global_mut::<Runtime>().owner;
      owner.tick();
      owner.active()
    }) else {
      break;
    };
    if active {
      cx.background_executor()
        .timer(Duration::from_millis(10))
        .await;
    } else {
      wake.notified().await;
    }
  });
  cx.set_global(Runtime {
    owner,
    _poll: task,
    viewers: BTreeMap::new(),
  });
  Ok(())
}

pub fn open(id: &str, title: &str, cx: &mut App) -> anyhow::Result<()> {
  let runtime = cx.global_mut::<Runtime>();
  if !runtime.viewers.contains_key(id) {
    let display = runtime.owner.display(id)?;
    let viewer = viewer::Viewer::new(display, title)?;
    runtime.viewers.insert(id.into(), viewer);
  }
  if let Some(viewer) = runtime.viewers.get_mut(id) {
    if !viewer.attached() {
      viewer.attach(runtime.owner.display(id)?)?;
    }
  }
  runtime
    .viewers
    .get(id)
    .ok_or_else(|| anyhow::anyhow!("VM viewer is unavailable"))?
    .show();
  Ok(())
}

pub fn owns(id: &str, cx: &App) -> bool {
  cx.has_global::<Runtime>() && cx.global::<Runtime>().owner.state(id).is_ok()
}

pub fn admit(
  prepared: host::VirtualLinuxPrepared,
  cx: &mut App,
) -> anyhow::Result<host::VirtualMachineAdmission> {
  let main = host::VirtualMachineThread::new()
    .ok_or_else(|| anyhow::anyhow!("VM admission requires the main thread"))?;
  prepared.admit(main, &mut cx.global_mut::<Runtime>().owner)
}

pub fn installer(id: &str, cx: &App) -> bool {
  cx.has_global::<Runtime>() && cx.global::<Runtime>().owner.installer(id).unwrap_or(false)
}

pub fn retire_installer(id: &str, cx: &mut App) -> anyhow::Result<()> {
  let runtime = cx.global_mut::<Runtime>();
  anyhow::ensure!(runtime.owner.installer(id)?, "This VM is not an installer");
  runtime.owner.can_replace(id)?;
  if let Some(viewer) = runtime.viewers.get_mut(id) {
    viewer.detach();
  }
  runtime.owner.retire(id)
}
