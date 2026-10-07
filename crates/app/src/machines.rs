use gpui::{App, Global, Task};
use host::{Host, VirtualMachineOwner};
use std::time::Duration;

struct Runtime {
  owner: VirtualMachineOwner,
  _poll: Task<()>,
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
  cx.set_global(Runtime { owner, _poll: task });
  Ok(())
}
