use super::Runtime;
use gpui::App;
use host::{VirtualMacAdmission, VirtualMacLaunch, VirtualMacPhase, VirtualMachineThread};

pub fn begin(id: &str, cx: &mut App) -> anyhow::Result<()> {
  let runtime = cx.global_mut::<Runtime>();
  anyhow::ensure!(
    !runtime.pending.contains(id),
    "VM setup is already in progress"
  );
  runtime.pending.insert(id.into());
  runtime
    .messages
    .insert(id.into(), VirtualMacPhase::Inspecting.message());
  Ok(())
}

pub fn phase(id: &str, phase: VirtualMacPhase, cx: &mut App) {
  let runtime = cx.global_mut::<Runtime>();
  if runtime.pending.contains(id) && !runtime.cancelling.contains(id) {
    runtime.messages.insert(id.into(), phase.message());
  }
}

pub fn cancelling(id: &str, cx: &mut App) {
  let runtime = cx.global_mut::<Runtime>();
  runtime.cancelling.insert(id.into());
  runtime
    .messages
    .insert(id.into(), "Cancelling macOS setup…".into());
}

pub fn failed_cancel(id: &str, cx: &mut App) {
  cx.global_mut::<Runtime>().cancelling.remove(id);
}

pub fn admit(launch: VirtualMacLaunch, cx: &mut App) -> anyhow::Result<VirtualMacAdmission> {
  let main = VirtualMachineThread::new()
    .ok_or_else(|| anyhow::anyhow!("VM admission requires the main thread"))?;
  launch.admit(main, &mut cx.global_mut::<Runtime>().owner)
}

pub fn ready(id: &str, cx: &App) -> bool {
  cx.global::<Runtime>()
    .owner
    .inspect(id)
    .is_ok_and(|status| status.mac_ready)
}

pub fn retire(id: &str, cx: &mut App) -> anyhow::Result<()> {
  let runtime = cx.global_mut::<Runtime>();
  runtime.owner.can_replace(id)?;
  if let Some(viewer) = runtime.viewers.get_mut(id) {
    viewer.detach();
  }
  runtime.owner.retire(id)
}

pub fn finish(id: &str, result: &anyhow::Result<()>, cx: &mut App) {
  if result.is_err() && super::owns(id, cx) && cx.global::<Runtime>().owner.can_replace(id).is_ok()
  {
    let _ = retire(id, cx);
  }
  let runtime = cx.global_mut::<Runtime>();
  runtime.pending.remove(id);
  runtime.cancelling.remove(id);
  if let Err(error) = result {
    runtime.messages.insert(id.into(), format!("{error:#}"));
  } else {
    runtime.messages.remove(id);
  }
}
